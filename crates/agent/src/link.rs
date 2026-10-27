use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::Duration;

use tungstenite::{Error as WsError, Message, WebSocket};
use wirehaus_crypto::session::Session;
use wirehaus_protocol::{Channel, ControlMessage, Frame};

use crate::error::ServerError;

const MIN_READ_WAIT: Duration = Duration::from_millis(1);

#[derive(Debug)]
pub enum Incoming {
    Frame(Frame),
    Idle,
    Closed,
}

pub trait FrameSink {
    fn send_frame(&mut self, frame: &Frame) -> Result<usize, ServerError>;
}

impl FrameSink for Link {
    fn send_frame(&mut self, frame: &Frame) -> Result<usize, ServerError> {
        Link::send_frame(self, frame)
    }
}

pub struct Link {
    socket: WebSocket<TcpStream>,
    session: Session,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").finish_non_exhaustive()
    }
}

impl Link {
    #[must_use]
    pub fn new(socket: WebSocket<TcpStream>, session: Session) -> Self {
        Self { socket, session }
    }

    pub fn send_frame(&mut self, frame: &Frame) -> Result<usize, ServerError> {
        let mut total = 0;
        for record in self.session.seal(&frame.encode()?)? {
            total += record.len();
            self.socket.write(Message::Binary(record.into()))?;
        }
        Ok(total)
    }

    pub fn send_control(&mut self, message: ControlMessage) -> Result<usize, ServerError> {
        self.send_frame(&Frame::new(Channel::Control, 0, message.encode())?)
    }

    pub fn ping(&mut self) -> Result<(), ServerError> {
        match self.socket.write(Message::Ping(Vec::new().into())) {
            Err(WsError::Io(error)) if is_timeout(error.kind()) => Ok(()),
            other => other.map_err(ServerError::from),
        }
    }

    pub fn flush(&mut self) -> Result<(), ServerError> {
        match self.socket.flush() {
            Err(WsError::Io(error)) if is_timeout(error.kind()) => Ok(()),
            other => other.map_err(ServerError::from),
        }
    }

    pub fn set_write_timeout(&self, timeout: Duration) -> Result<(), ServerError> {
        Ok(self.socket.get_ref().set_write_timeout(Some(timeout))?)
    }

    pub fn receive(&mut self, wait: Duration) -> Result<Incoming, ServerError> {
        self.socket
            .get_ref()
            .set_read_timeout(Some(wait.max(MIN_READ_WAIT)))?;
        match self.socket.read() {
            Ok(Message::Binary(bytes)) => match self.session.open(&bytes)? {
                Some(message) => Ok(Incoming::Frame(Frame::decode(&message)?)),
                None => Ok(Incoming::Idle),
            },
            Ok(Message::Close(_)) | Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => {
                Ok(Incoming::Closed)
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => Ok(Incoming::Idle),
            Ok(_) => Err(ServerError::UnexpectedMessage),
            Err(WsError::Io(error)) if is_timeout(error.kind()) => Ok(Incoming::Idle),
            Err(error) => Err(error.into()),
        }
    }

    pub fn close(&mut self) {
        let _ = self.socket.close(None);
        let _ = self.socket.flush();
    }
}

pub fn is_timeout(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::WouldBlock | ErrorKind::TimedOut)
}
