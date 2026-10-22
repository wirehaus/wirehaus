use std::net::TcpStream;
use std::time::{Duration, Instant};

use tungstenite::{Error as WsError, Message, WebSocket};
use wirehaus_crypto::handshake::{Accepted, Established, Mode, Replied};
use wirehaus_crypto::keys::PublicKey;
use wirehaus_crypto::lockout::Denied;
use wirehaus_crypto::session::Limits;
use wirehaus_protocol::{AuthFailure, Channel, ControlMessage, Frame, HEADER_LEN, MAX_PAYLOAD_LEN};

use crate::error::ServerError;
use crate::link::{Incoming, Link, is_timeout};
use crate::state::AuthError;
use crate::store::Store;

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
pub const CODE_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_CODE_ATTEMPTS: u32 = 3;
pub const INBOUND_LIMIT: usize = wirehaus_protocol::MAX_VIEWER_MESSAGE;
pub const OUTBOUND_LIMIT: usize = HEADER_LEN + MAX_PAYLOAD_LEN;

#[derive(Debug)]
pub struct Identity {
    pub device: PublicKey,
    pub name: String,
}

#[derive(Debug)]
pub enum Outcome {
    Authenticated(Box<Link>, Identity),
    Refused,
}

pub fn negotiate(mut socket: WebSocket<TcpStream>, store: &Store) -> Result<Outcome, ServerError> {
    let Some(first) = read_binary(&mut socket, Instant::now() + HANDSHAKE_TIMEOUT)? else {
        return Ok(Outcome::Refused);
    };
    let (locked, agent, psk) = store.read(|state, now| {
        (
            state.is_locked(),
            state.agent_key().clone(),
            state.pairing_psk(now),
        )
    })?;
    if locked {
        store.log("refused_locked", "the agent is locked");
        close(&mut socket);
        return Ok(Outcome::Refused);
    }
    let limits = Limits {
        send: OUTBOUND_LIMIT,
        receive: INBOUND_LIMIT,
    };
    let accepted = match Accepted::begin(&first, &agent, psk.as_ref(), limits) {
        Ok(accepted) => accepted,
        Err(error) => {
            if first.first() == Some(&Mode::Pairing.byte()) {
                let _ = store.update(|state, _| state.record_pairing_failure());
            }
            store.log("handshake_failed", &error.to_string());
            close(&mut socket);
            return Ok(Outcome::Refused);
        }
    };
    match accepted.mode() {
        Mode::Reconnect => reconnect(socket, store, accepted),
        Mode::Pairing => pair(socket, store, accepted),
    }
}

fn reconnect(
    mut socket: WebSocket<TcpStream>,
    store: &Store,
    accepted: Accepted,
) -> Result<Outcome, ServerError> {
    let Some(device) = accepted.device() else {
        close(&mut socket);
        return Ok(Outcome::Refused);
    };
    let name = store.read(|state, _| state.device(&device).map(|known| known.name.clone()))?;
    let Some(name) = name else {
        store.log("unknown_device", &device.to_text());
        close(&mut socket);
        return Ok(Outcome::Refused);
    };
    let Replied::Established {
        message,
        established,
    } = accepted.reply()?
    else {
        close(&mut socket);
        return Ok(Outcome::Refused);
    };
    socket.send(Message::Binary(message.into()))?;
    authenticate(
        Link::new(socket, established.session),
        store,
        device,
        name,
        false,
    )
}

fn pair(
    mut socket: WebSocket<TcpStream>,
    store: &Store,
    accepted: Accepted,
) -> Result<Outcome, ServerError> {
    let Replied::AwaitingFinal { message, pending } = accepted.reply()? else {
        close(&mut socket);
        return Ok(Outcome::Refused);
    };
    socket.send(Message::Binary(message.into()))?;
    let Some(last) = read_binary(&mut socket, Instant::now() + HANDSHAKE_TIMEOUT)? else {
        let _ = store.update(|state, _| state.record_pairing_failure());
        store.log("pairing_failed", "no final handshake message");
        return Ok(Outcome::Refused);
    };
    let Established {
        session,
        device,
        device_name,
    } = match pending.finish(&last) {
        Ok(established) => established,
        Err(error) => {
            let _ = store.update(|state, _| state.record_pairing_failure());
            store.log("pairing_failed", &error.to_string());
            close(&mut socket);
            return Ok(Outcome::Refused);
        }
    };
    let name = device_name.unwrap_or_default();
    authenticate(Link::new(socket, session), store, device, name, true)
}

fn authenticate(
    mut link: Link,
    store: &Store,
    device: PublicKey,
    name: String,
    pairing: bool,
) -> Result<Outcome, ServerError> {
    if !pairing && let Err(denied) = store.read(|state, now| state.check_device(&device, now))? {
        return Ok(refuse(&mut link, store, &name, denied_failure(denied)));
    }
    link.send_control(ControlMessage::AuthRequired)?;
    link.flush()?;
    for _ in 0..MAX_CODE_ATTEMPTS {
        let Some(code) = read_code(&mut link)? else {
            store.log("totp_timeout", &name);
            link.close();
            return Ok(Outcome::Refused);
        };
        let verdict = store.update(|state, now| {
            let verdict = state.verify_totp((!pairing).then_some(&device), &code, now);
            if pairing && verdict.is_err() {
                state.record_pairing_failure();
            }
            verdict
        })?;
        match verdict {
            Ok(()) => return succeed(link, store, device, name, pairing),
            Err(AuthError::Denied(denied)) => {
                return Ok(refuse(&mut link, store, &name, denied_failure(denied)));
            }
            Err(AuthError::WrongCode) => {
                store.log("totp_failed", &name);
                if !pairing
                    && let Err(denied) =
                        store.read(|state, now| state.check_device(&device, now))?
                {
                    return Ok(refuse(&mut link, store, &name, denied_failure(denied)));
                }
                link.send_control(failure_message((AuthFailure::WrongCode, 0)))?;
                link.flush()?;
            }
        }
    }
    link.close();
    Ok(Outcome::Refused)
}

fn succeed(
    mut link: Link,
    store: &Store,
    device: PublicKey,
    name: String,
    pairing: bool,
) -> Result<Outcome, ServerError> {
    if pairing {
        let added = store.update(|state, now| {
            let added = state.add_device(device, &name, now);
            state.close_pairing();
            added
        })?;
        if let Err(error) = added {
            store.log("pairing_failed", &error.to_string());
            link.close();
            return Ok(Outcome::Refused);
        }
        store.log("paired", &format!("{name} {}", device.to_text()));
    }
    store.log("auth_ok", &name);
    Ok(Outcome::Authenticated(
        Box::new(link),
        Identity { device, name },
    ))
}

fn refuse(link: &mut Link, store: &Store, name: &str, failure: (AuthFailure, u32)) -> Outcome {
    store.log(
        match failure.0 {
            AuthFailure::Suspended => "device_suspended",
            _ => "device_locked",
        },
        name,
    );
    let _ = link.send_control(failure_message(failure));
    let _ = link.flush();
    link.close();
    Outcome::Refused
}

fn failure_message((reason, retry_after): (AuthFailure, u32)) -> ControlMessage {
    ControlMessage::AuthFailed {
        reason,
        retry_after,
    }
}

fn denied_failure(denied: Denied) -> (AuthFailure, u32) {
    match denied {
        Denied::Locked { retry_after } => (
            AuthFailure::Locked,
            u32::try_from(retry_after).unwrap_or(u32::MAX),
        ),
        Denied::Suspended => (AuthFailure::Suspended, 0),
    }
}

fn read_code(link: &mut Link) -> Result<Option<String>, ServerError> {
    let deadline = Instant::now() + CODE_TIMEOUT;
    loop {
        let wait = deadline.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            return Ok(None);
        }
        match link.receive(wait)? {
            Incoming::Frame(frame) => return code_of(&frame).map(Some),
            Incoming::Idle => {}
            Incoming::Closed => return Ok(None),
        }
    }
}

fn code_of(frame: &Frame) -> Result<String, ServerError> {
    if frame.channel != Channel::Control {
        return Err(ServerError::UnexpectedChannel(frame.channel));
    }
    match ControlMessage::decode(&frame.payload)? {
        ControlMessage::Totp { code } => Ok(code.iter().map(|&digit| char::from(digit)).collect()),
        _ => Err(ServerError::UnexpectedMessage),
    }
}

fn read_binary(
    socket: &mut WebSocket<TcpStream>,
    deadline: Instant,
) -> Result<Option<Vec<u8>>, ServerError> {
    loop {
        let wait = deadline.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            return Ok(None);
        }
        socket
            .get_ref()
            .set_read_timeout(Some(wait.max(Duration::from_millis(1))))?;
        match socket.read() {
            Ok(Message::Binary(bytes)) => return Ok(Some(bytes.to_vec())),
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            Ok(Message::Close(_)) | Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => {
                return Ok(None);
            }
            Ok(_) => return Err(ServerError::UnexpectedMessage),
            Err(WsError::Io(error)) if is_timeout(error.kind()) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn close(socket: &mut WebSocket<TcpStream>) {
    let _ = socket.close(None);
    let _ = socket.flush();
}
