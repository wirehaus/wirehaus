use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tungstenite::{Error as WsError, Message, WebSocket};
use wirehaus_capture::{Bitmap, Rect, all_tiles, changed_tiles, coalesce};
use wirehaus_encode::{JpegEncoder, TileEncoder};
use wirehaus_input::{InputError, InputSink};
use wirehaus_protocol::{Channel, DesktopMessage, Frame, InputEvent, ProtocolError};

use crate::error::ServerError;
use crate::platform::Backend;

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const MIN_READ_WAIT: Duration = Duration::from_millis(1);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGES_PER_TICK: usize = 256;

pub fn run(
    mut socket: WebSocket<TcpStream>,
    mut backend: Backend,
    mut encoder: JpegEncoder,
    stop: &AtomicBool,
) -> Result<(), ServerError> {
    socket.get_ref().set_write_timeout(Some(WRITE_TIMEOUT))?;
    let mut previous: Option<Bitmap> = None;
    while !stop.load(Ordering::Relaxed) {
        let deadline = Instant::now() + FRAME_INTERVAL;
        let current = backend.screen.grab()?;
        send_changes(&mut socket, &mut encoder, previous.as_ref(), &current)?;
        previous = Some(current);
        socket.flush().or_else(ignore_would_block)?;
        if !receive(&mut socket, backend.input.as_mut(), deadline)? {
            return Ok(());
        }
    }
    let _ = socket.close(None);
    Ok(())
}

fn send_changes(
    socket: &mut WebSocket<TcpStream>,
    encoder: &mut JpegEncoder,
    previous: Option<&Bitmap>,
    current: &Bitmap,
) -> Result<(), ServerError> {
    let tiles = match previous {
        Some(old) if same_size(old, current) => changed_tiles(old, current),
        _ => {
            send_message(
                socket,
                &DesktopMessage::Resize {
                    width: current.width(),
                    height: current.height(),
                },
            )?;
            all_tiles(current)
        }
    };
    for rect in coalesce(&tiles) {
        send_rect(socket, encoder, current, rect)?;
    }
    Ok(())
}

fn same_size(a: &Bitmap, b: &Bitmap) -> bool {
    a.width() == b.width() && a.height() == b.height()
}

fn send_rect(
    socket: &mut WebSocket<TcpStream>,
    encoder: &mut JpegEncoder,
    bitmap: &Bitmap,
    rect: Rect,
) -> Result<(), ServerError> {
    let message = DesktopMessage::Tile {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        codec: encoder.codec(),
        data: encoder.encode(bitmap, rect)?,
    };
    match Frame::new(Channel::Desktop, 0, message.encode()) {
        Ok(frame) => send_frame(socket, &frame),
        Err(ProtocolError::PayloadTooLarge { .. }) => match split(rect) {
            Some(halves) => halves
                .into_iter()
                .try_for_each(|half| send_rect(socket, encoder, bitmap, half)),
            None => Err(ProtocolError::PayloadTooLarge {
                len: usize::MAX,
                max: wirehaus_protocol::MAX_PAYLOAD_LEN,
            }
            .into()),
        },
        Err(error) => Err(error.into()),
    }
}

fn split(rect: Rect) -> Option<[Rect; 2]> {
    if rect.width > 1 {
        let left = rect.width / 2;
        Some([
            Rect {
                width: left,
                ..rect
            },
            Rect {
                x: rect.x + left,
                width: rect.width - left,
                ..rect
            },
        ])
    } else if rect.height > 1 {
        let top = rect.height / 2;
        Some([
            Rect {
                height: top,
                ..rect
            },
            Rect {
                y: rect.y + top,
                height: rect.height - top,
                ..rect
            },
        ])
    } else {
        None
    }
}

fn send_message(
    socket: &mut WebSocket<TcpStream>,
    message: &DesktopMessage,
) -> Result<(), ServerError> {
    send_frame(socket, &Frame::new(Channel::Desktop, 0, message.encode())?)
}

fn send_frame(socket: &mut WebSocket<TcpStream>, frame: &Frame) -> Result<(), ServerError> {
    socket
        .write(Message::Binary(frame.encode()?.into()))
        .map_err(ServerError::from)
}

fn receive(
    socket: &mut WebSocket<TcpStream>,
    input: &mut dyn InputSink,
    deadline: Instant,
) -> Result<bool, ServerError> {
    let mut handled = 0;
    while handled < MAX_MESSAGES_PER_TICK {
        let wait = deadline.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            break;
        }
        socket
            .get_ref()
            .set_read_timeout(Some(wait.max(MIN_READ_WAIT)))?;
        match socket.read() {
            Ok(Message::Binary(bytes)) => {
                handle_binary(&bytes, input)?;
                handled += 1;
            }
            Ok(Message::Close(_)) | Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => {
                return Ok(false);
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            Ok(_) => return Err(ServerError::UnexpectedMessage),
            Err(WsError::Io(e)) if is_timeout(e.kind()) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(true)
}

fn handle_binary(bytes: &[u8], input: &mut dyn InputSink) -> Result<(), ServerError> {
    let frame = Frame::decode(bytes)?;
    if frame.channel != Channel::Desktop {
        return Err(ServerError::UnexpectedChannel(frame.channel));
    }
    let event = InputEvent::decode(&frame.payload)?;
    match input.send(event) {
        Ok(()) | Err(InputError::PointerOutOfRange { .. } | InputError::KeyOutOfRange(_)) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn is_timeout(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

fn ignore_would_block(error: WsError) -> Result<(), WsError> {
    match error {
        WsError::Io(e) if is_timeout(e.kind()) => Ok(()),
        other => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirehaus_protocol::Button;

    #[derive(Default)]
    struct Recorder {
        events: Vec<InputEvent>,
        fail_with: Option<fn() -> InputError>,
    }

    impl InputSink for Recorder {
        fn send(&mut self, event: InputEvent) -> Result<(), InputError> {
            if let Some(make) = self.fail_with {
                return Err(make());
            }
            self.events.push(event);
            Ok(())
        }
    }

    fn input_frame(channel: Channel, event: InputEvent) -> Vec<u8> {
        Frame::new(channel, 0, event.encode())
            .unwrap()
            .encode()
            .unwrap()
    }

    #[test]
    fn forwards_a_valid_input_event() {
        let mut sink = Recorder::default();
        let event = InputEvent::Button {
            button: Button::Left,
            pressed: true,
        };
        handle_binary(&input_frame(Channel::Desktop, event), &mut sink).unwrap();
        assert_eq!(sink.events, [event]);
    }

    #[test]
    fn rejects_input_on_another_channel() {
        let mut sink = Recorder::default();
        let bytes = input_frame(Channel::Terminal, InputEvent::PointerMove { x: 1, y: 1 });
        assert!(matches!(
            handle_binary(&bytes, &mut sink),
            Err(ServerError::UnexpectedChannel(Channel::Terminal))
        ));
        assert_eq!(sink.events, []);
    }

    #[test]
    fn rejects_malformed_frames_and_events() {
        let mut sink = Recorder::default();
        assert!(handle_binary(&[1, 2, 3], &mut sink).is_err());
        let bad_event = Frame::new(Channel::Desktop, 0, vec![99])
            .unwrap()
            .encode()
            .unwrap();
        assert!(handle_binary(&bad_event, &mut sink).is_err());
        assert_eq!(sink.events, []);
    }

    #[test]
    fn ignores_out_of_range_events_but_not_other_failures() {
        let bytes = input_frame(Channel::Desktop, InputEvent::PointerMove { x: 9, y: 9 });
        let mut out_of_range = Recorder {
            fail_with: Some(|| InputError::PointerOutOfRange { x: 9, y: 9 }),
            ..Recorder::default()
        };
        assert!(handle_binary(&bytes, &mut out_of_range).is_ok());
        let mut broken = Recorder {
            fail_with: Some(|| InputError::Request("gone".into())),
            ..Recorder::default()
        };
        assert!(handle_binary(&bytes, &mut broken).is_err());
    }

    #[test]
    fn splits_wide_regions_before_tall_ones() {
        let rect = Rect {
            x: 10,
            y: 20,
            width: 5,
            height: 8,
        };
        let [left, right] = split(rect).unwrap();
        assert_eq!((left.x, left.width), (10, 2));
        assert_eq!((right.x, right.width), (12, 3));
        assert_eq!(left.height, 8);
        let column = Rect { width: 1, ..rect };
        let [top, bottom] = split(column).unwrap();
        assert_eq!((top.y, top.height), (20, 4));
        assert_eq!((bottom.y, bottom.height), (24, 4));
    }

    #[test]
    fn a_single_pixel_cannot_be_split() {
        assert!(
            split(Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1
            })
            .is_none()
        );
    }
}
