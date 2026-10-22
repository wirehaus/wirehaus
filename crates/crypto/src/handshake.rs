use snow::params::NoiseParams;
use snow::{Builder, HandshakeState};

use crate::error::CryptoError;
use crate::keys::{PublicKey, StaticKey};
use crate::pairing::{PSK_BYTES, PairingCode};
use crate::session::Session;

pub const MAX_HANDSHAKE_MESSAGE: usize = 1024;
pub const MAX_DEVICE_NAME: usize = 64;

const RECONNECT_PARAMS: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
const PAIRING_PARAMS: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"wirehaus/1";
const PSK_POSITION: u8 = 3;
const WORK_BUFFER: usize = 65535;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Reconnect,
    Pairing,
}

impl Mode {
    #[must_use]
    pub fn byte(self) -> u8 {
        match self {
            Mode::Reconnect => 0,
            Mode::Pairing => 1,
        }
    }

    pub fn from_byte(byte: u8) -> Result<Self, CryptoError> {
        match byte {
            0 => Ok(Mode::Reconnect),
            1 => Ok(Mode::Pairing),
            other => Err(CryptoError::UnknownMode(other)),
        }
    }

    fn params(self) -> Result<NoiseParams, CryptoError> {
        let text = match self {
            Mode::Reconnect => RECONNECT_PARAMS,
            Mode::Pairing => PAIRING_PARAMS,
        };
        Ok(text.parse()?)
    }

    fn prologue(self) -> Vec<u8> {
        let mut out = PROLOGUE.to_vec();
        out.push(self.byte());
        out
    }
}

macro_rules! opaque_debug {
    ($($name:ident),+) => {
        $(impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($name)).finish_non_exhaustive()
            }
        })+
    };
}

pub struct ViewerHandshake {
    state: HandshakeState,
    mode: Mode,
    device_name: String,
    max_message: usize,
}

pub struct ViewerOutcome {
    pub session: Session,
    pub last_message: Option<Vec<u8>>,
}

impl ViewerHandshake {
    pub fn reconnect(
        device: &StaticKey,
        agent: &PublicKey,
        max_message: usize,
    ) -> Result<(Self, Vec<u8>), CryptoError> {
        let mode = Mode::Reconnect;
        let state = Builder::new(mode.params()?)
            .prologue(&mode.prologue())?
            .local_private_key(device.private_bytes())?
            .remote_public_key(agent.as_bytes())?
            .build_initiator()?;
        Self::start(state, mode, String::new(), max_message)
    }

    pub fn pairing(
        device: &StaticKey,
        code: &PairingCode,
        device_name: &str,
        max_message: usize,
    ) -> Result<(Self, Vec<u8>), CryptoError> {
        let name = validate_device_name(device_name)?;
        let mode = Mode::Pairing;
        let psk = code.psk();
        let state = Builder::new(mode.params()?)
            .prologue(&mode.prologue())?
            .local_private_key(device.private_bytes())?
            .psk(PSK_POSITION, &psk)?
            .build_initiator()?;
        Self::start(state, mode, name, max_message)
    }

    fn start(
        mut state: HandshakeState,
        mode: Mode,
        device_name: String,
        max_message: usize,
    ) -> Result<(Self, Vec<u8>), CryptoError> {
        let mut buffer = vec![0_u8; WORK_BUFFER];
        let length = state.write_message(&[], &mut buffer)?;
        let mut first = Vec::with_capacity(1 + length);
        first.push(mode.byte());
        first.extend_from_slice(&buffer[..length]);
        Ok((
            Self {
                state,
                mode,
                device_name,
                max_message,
            },
            first,
        ))
    }

    pub fn complete(mut self, reply: &[u8]) -> Result<ViewerOutcome, CryptoError> {
        check_size(reply)?;
        let mut payload = vec![0_u8; WORK_BUFFER];
        self.state.read_message(reply, &mut payload)?;
        let last_message = match self.mode {
            Mode::Reconnect => None,
            Mode::Pairing => {
                let mut buffer = vec![0_u8; WORK_BUFFER];
                let length = self
                    .state
                    .write_message(self.device_name.as_bytes(), &mut buffer)?;
                buffer.truncate(length);
                Some(buffer)
            }
        };
        let remote = remote_static(&self.state)?;
        let transport = self.state.into_transport_mode()?;
        Ok(ViewerOutcome {
            session: Session::new(transport, remote, self.max_message),
            last_message,
        })
    }
}

pub struct Accepted {
    state: HandshakeState,
    mode: Mode,
    device: Option<PublicKey>,
    max_message: usize,
}

pub struct Established {
    pub session: Session,
    pub device: PublicKey,
    pub device_name: Option<String>,
}

pub enum Replied {
    Established {
        message: Vec<u8>,
        established: Box<Established>,
    },
    AwaitingFinal {
        message: Vec<u8>,
        pending: Box<AwaitingFinal>,
    },
}

pub struct AwaitingFinal {
    state: HandshakeState,
    max_message: usize,
}

impl Accepted {
    pub fn begin(
        first: &[u8],
        agent: &StaticKey,
        pairing_psk: Option<&[u8; PSK_BYTES]>,
        max_message: usize,
    ) -> Result<Self, CryptoError> {
        check_size(first)?;
        let (&mode_byte, message) = first
            .split_first()
            .ok_or(CryptoError::InvalidRecord("empty handshake message"))?;
        let mode = Mode::from_byte(mode_byte)?;
        let prologue = mode.prologue();
        let builder = Builder::new(mode.params()?)
            .prologue(&prologue)?
            .local_private_key(agent.private_bytes())?;
        let mut state = match mode {
            Mode::Reconnect => builder.build_responder()?,
            Mode::Pairing => builder
                .psk(
                    PSK_POSITION,
                    pairing_psk.ok_or(CryptoError::PairingNotOpen)?,
                )?
                .build_responder()?,
        };
        let mut payload = vec![0_u8; WORK_BUFFER];
        state.read_message(message, &mut payload)?;
        let device = match mode {
            Mode::Reconnect => Some(remote_static(&state)?),
            Mode::Pairing => None,
        };
        Ok(Self {
            state,
            mode,
            device,
            max_message,
        })
    }

    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    #[must_use]
    pub fn device(&self) -> Option<PublicKey> {
        self.device
    }

    pub fn reply(mut self) -> Result<Replied, CryptoError> {
        let mut buffer = vec![0_u8; WORK_BUFFER];
        let length = self.state.write_message(&[], &mut buffer)?;
        buffer.truncate(length);
        match self.device {
            Some(device) => {
                let transport = self.state.into_transport_mode()?;
                Ok(Replied::Established {
                    message: buffer,
                    established: Box::new(Established {
                        session: Session::new(transport, device, self.max_message),
                        device,
                        device_name: None,
                    }),
                })
            }
            None => Ok(Replied::AwaitingFinal {
                message: buffer,
                pending: Box::new(AwaitingFinal {
                    state: self.state,
                    max_message: self.max_message,
                }),
            }),
        }
    }
}

impl AwaitingFinal {
    pub fn finish(mut self, message: &[u8]) -> Result<Established, CryptoError> {
        check_size(message)?;
        let mut payload = vec![0_u8; WORK_BUFFER];
        let length = self.state.read_message(message, &mut payload)?;
        let name = std::str::from_utf8(&payload[..length])
            .map_err(|_| CryptoError::InvalidDeviceName)
            .and_then(validate_device_name)?;
        let device = remote_static(&self.state)?;
        let transport = self.state.into_transport_mode()?;
        Ok(Established {
            session: Session::new(transport, device, self.max_message),
            device,
            device_name: Some(name),
        })
    }
}

pub fn validate_device_name(name: &str) -> Result<String, CryptoError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_DEVICE_NAME
        || trimmed.chars().any(char::is_control)
    {
        return Err(CryptoError::InvalidDeviceName);
    }
    Ok(trimmed.to_owned())
}

fn check_size(message: &[u8]) -> Result<(), CryptoError> {
    if message.len() > MAX_HANDSHAKE_MESSAGE {
        return Err(CryptoError::HandshakeTooLarge {
            len: message.len(),
            max: MAX_HANDSHAKE_MESSAGE,
        });
    }
    Ok(())
}

fn remote_static(state: &HandshakeState) -> Result<PublicKey, CryptoError> {
    state
        .get_remote_static()
        .ok_or(CryptoError::InvalidRecord("the peer sent no static key"))
        .and_then(PublicKey::from_bytes)
}

opaque_debug!(
    ViewerHandshake,
    ViewerOutcome,
    Accepted,
    Established,
    AwaitingFinal
);

impl std::fmt::Debug for Replied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Replied::Established { .. } => "Replied::Established",
            Replied::AwaitingFinal { .. } => "Replied::AwaitingFinal",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 1 << 20;

    fn keys() -> (StaticKey, StaticKey) {
        (
            StaticKey::generate().unwrap(),
            StaticKey::generate().unwrap(),
        )
    }

    fn reconnect(agent: &StaticKey, device: &StaticKey) -> (Session, Established) {
        let (handshake, first) =
            ViewerHandshake::reconnect(device, &agent.public(), LIMIT).unwrap();
        let accepted = Accepted::begin(&first, agent, None, LIMIT).unwrap();
        let Replied::Established {
            message,
            established,
        } = accepted.reply().unwrap()
        else {
            panic!("a reconnection finishes in two messages");
        };
        let outcome = handshake.complete(&message).unwrap();
        assert!(outcome.last_message.is_none());
        (outcome.session, *established)
    }

    fn pair(
        agent: &StaticKey,
        device: &StaticKey,
        viewer_code: &PairingCode,
        agent_code: &PairingCode,
        name: &str,
    ) -> Result<(Session, Established), CryptoError> {
        let (handshake, first) = ViewerHandshake::pairing(device, viewer_code, name, LIMIT)?;
        let accepted = Accepted::begin(&first, agent, Some(&agent_code.psk()), LIMIT)?;
        let Replied::AwaitingFinal { message, pending } = accepted.reply()? else {
            panic!("pairing needs a third message");
        };
        let outcome = handshake.complete(&message)?;
        let last = outcome
            .last_message
            .expect("pairing ends with a third message");
        let established = pending.finish(&last)?;
        Ok((outcome.session, established))
    }

    #[test]
    fn reconnecting_gives_both_ends_a_working_session_and_each_other_s_identity() {
        let (agent, device) = keys();
        let (mut viewer, mut established) = reconnect(&agent, &device);
        assert_eq!(established.device, device.public());
        assert_eq!(viewer.remote(), agent.public());
        assert_eq!(established.session.remote(), device.public());
        let records = viewer.seal(b"ping").unwrap();
        assert_eq!(
            established.session.open(&records[0]).unwrap().unwrap(),
            b"ping"
        );
        let records = established.session.seal(b"pong").unwrap();
        assert_eq!(viewer.open(&records[0]).unwrap().unwrap(), b"pong");
    }

    #[test]
    fn the_agent_learns_the_device_before_it_replies() {
        let (agent, device) = keys();
        let (_handshake, first) =
            ViewerHandshake::reconnect(&device, &agent.public(), LIMIT).unwrap();
        let accepted = Accepted::begin(&first, &agent, None, LIMIT).unwrap();
        assert_eq!(accepted.mode(), Mode::Reconnect);
        assert_eq!(accepted.device(), Some(device.public()));
    }

    #[test]
    fn a_viewer_that_pinned_another_agent_key_cannot_complete_a_handshake() {
        let (agent, device) = keys();
        let impostor = StaticKey::generate().unwrap();
        let (_handshake, first) =
            ViewerHandshake::reconnect(&device, &impostor.public(), LIMIT).unwrap();
        assert!(matches!(
            Accepted::begin(&first, &agent, None, LIMIT),
            Err(CryptoError::Noise(_))
        ));
    }

    #[test]
    fn an_impostor_agent_cannot_answer_a_viewer_that_pinned_the_real_one() {
        let (agent, device) = keys();
        let impostor = StaticKey::generate().unwrap();
        let (handshake, first) =
            ViewerHandshake::reconnect(&device, &agent.public(), LIMIT).unwrap();
        assert!(Accepted::begin(&first, &impostor, None, LIMIT).is_err());
        let (other, first_other) =
            ViewerHandshake::reconnect(&device, &impostor.public(), LIMIT).unwrap();
        let accepted = Accepted::begin(&first_other, &impostor, None, LIMIT).unwrap();
        let Replied::Established { message, .. } = accepted.reply().unwrap() else {
            panic!("unexpected reply");
        };
        assert!(handshake.complete(&message).is_err());
        drop(other);
    }

    #[test]
    fn a_replayed_first_message_gives_the_attacker_no_session() {
        let (agent, device) = keys();
        let (_viewer, first) = ViewerHandshake::reconnect(&device, &agent.public(), LIMIT).unwrap();
        let accepted = Accepted::begin(&first, &agent, None, LIMIT).unwrap();
        let Replied::Established { established, .. } = accepted.reply().unwrap() else {
            panic!("unexpected reply");
        };
        let mut session = established.session;
        let forged = [0x55_u8; 64];
        assert!(session.open(&forged).is_err());
    }

    #[test]
    fn pairing_teaches_each_side_the_other_s_key_and_the_device_name() {
        let (agent, device) = keys();
        let code = PairingCode::generate().unwrap();
        let (mut viewer, mut established) =
            pair(&agent, &device, &code, &code, "  Aula 3, PC 14  ").unwrap();
        assert_eq!(viewer.remote(), agent.public());
        assert_eq!(established.device, device.public());
        assert_eq!(established.device_name.as_deref(), Some("Aula 3, PC 14"));
        let records = viewer.seal(b"paired").unwrap();
        assert_eq!(
            established.session.open(&records[0]).unwrap().unwrap(),
            b"paired"
        );
    }

    #[test]
    fn a_wrong_pairing_code_is_rejected_by_the_agent() {
        let (agent, device) = keys();
        let right = PairingCode::generate().unwrap();
        let wrong = PairingCode::generate().unwrap();
        assert!(matches!(
            pair(&agent, &device, &wrong, &right, "device"),
            Err(CryptoError::Noise(_))
        ));
    }

    #[test]
    fn pairing_is_refused_when_no_code_is_open() {
        let (agent, device) = keys();
        let code = PairingCode::generate().unwrap();
        let (_handshake, first) =
            ViewerHandshake::pairing(&device, &code, "device", LIMIT).unwrap();
        assert_eq!(
            Accepted::begin(&first, &agent, None, LIMIT).unwrap_err(),
            CryptoError::PairingNotOpen
        );
    }

    #[test]
    fn changing_the_mode_byte_breaks_the_handshake() {
        let (agent, device) = keys();
        let code = PairingCode::generate().unwrap();
        let (_h, mut reconnect_first) =
            ViewerHandshake::reconnect(&device, &agent.public(), LIMIT).unwrap();
        reconnect_first[0] = Mode::Pairing.byte();
        assert!(Accepted::begin(&reconnect_first, &agent, Some(&code.psk()), LIMIT).is_err());

        let (_h, mut pairing_first) = ViewerHandshake::pairing(&device, &code, "d", LIMIT).unwrap();
        pairing_first[0] = Mode::Reconnect.byte();
        assert!(Accepted::begin(&pairing_first, &agent, None, LIMIT).is_err());
    }

    #[test]
    fn an_unknown_mode_is_rejected() {
        let (agent, _device) = keys();
        assert_eq!(
            Accepted::begin(&[9, 1, 2, 3], &agent, None, LIMIT).unwrap_err(),
            CryptoError::UnknownMode(9)
        );
    }

    #[test]
    fn malformed_first_messages_are_rejected() {
        let (agent, device) = keys();
        assert!(Accepted::begin(&[], &agent, None, LIMIT).is_err());
        assert!(Accepted::begin(&[0], &agent, None, LIMIT).is_err());
        assert!(Accepted::begin(&[0; 40], &agent, None, LIMIT).is_err());
        let (_h, first) = ViewerHandshake::reconnect(&device, &agent.public(), LIMIT).unwrap();
        for cut in [1, 2, 33, first.len() - 1] {
            assert!(
                Accepted::begin(&first[..cut], &agent, None, LIMIT).is_err(),
                "cut {cut}"
            );
        }
        let mut oversized = first.clone();
        oversized.resize(MAX_HANDSHAKE_MESSAGE + 1, 0);
        assert!(matches!(
            Accepted::begin(&oversized, &agent, None, LIMIT),
            Err(CryptoError::HandshakeTooLarge { .. })
        ));
    }

    #[test]
    fn a_tampered_first_message_is_rejected() {
        let (agent, device) = keys();
        let (_h, first) = ViewerHandshake::reconnect(&device, &agent.public(), LIMIT).unwrap();
        for position in [1, 20, 40, first.len() - 1] {
            let mut tampered = first.clone();
            tampered[position] ^= 0x80;
            assert!(
                Accepted::begin(&tampered, &agent, None, LIMIT).is_err(),
                "byte {position}"
            );
        }
    }

    #[test]
    fn a_tampered_reply_is_rejected_by_the_viewer() {
        let (agent, device) = keys();
        let (handshake, first) =
            ViewerHandshake::reconnect(&device, &agent.public(), LIMIT).unwrap();
        let accepted = Accepted::begin(&first, &agent, None, LIMIT).unwrap();
        let Replied::Established { mut message, .. } = accepted.reply().unwrap() else {
            panic!("unexpected reply");
        };
        let last = message.len() - 1;
        message[last] ^= 1;
        assert!(handshake.complete(&message).is_err());
    }

    #[test]
    fn a_tampered_final_pairing_message_is_rejected() {
        let (agent, device) = keys();
        let code = PairingCode::generate().unwrap();
        let (handshake, first) = ViewerHandshake::pairing(&device, &code, "device", LIMIT).unwrap();
        let accepted = Accepted::begin(&first, &agent, Some(&code.psk()), LIMIT).unwrap();
        let Replied::AwaitingFinal { message, pending } = accepted.reply().unwrap() else {
            panic!("unexpected reply");
        };
        let mut last = handshake.complete(&message).unwrap().last_message.unwrap();
        let end = last.len() - 1;
        last[end] ^= 1;
        assert!(pending.finish(&last).is_err());
    }

    #[test]
    fn two_handshakes_between_the_same_keys_use_different_session_keys() {
        let (agent, device) = keys();
        let (mut first, _a) = reconnect(&agent, &device);
        let (mut second, _b) = reconnect(&agent, &device);
        assert_ne!(first.seal(b"same").unwrap(), second.seal(b"same").unwrap());
    }

    #[test]
    fn device_names_are_validated() {
        assert_eq!(validate_device_name("  Casa  ").unwrap(), "Casa");
        assert_eq!(validate_device_name("Aula ñ 3").unwrap(), "Aula ñ 3");
        for name in ["", "   ", "bad\nname", "tab\there", "nul\0"] {
            assert_eq!(
                validate_device_name(name),
                Err(CryptoError::InvalidDeviceName),
                "{name:?}"
            );
        }
        assert!(validate_device_name(&"a".repeat(MAX_DEVICE_NAME)).is_ok());
        assert!(validate_device_name(&"a".repeat(MAX_DEVICE_NAME + 1)).is_err());
    }

    #[test]
    fn pairing_with_an_invalid_name_never_starts() {
        let (_agent, device) = keys();
        let code = PairingCode::generate().unwrap();
        assert!(ViewerHandshake::pairing(&device, &code, "", LIMIT).is_err());
    }

    #[test]
    fn debug_output_never_shows_secret_material() {
        let (agent, device) = keys();
        let (handshake, _first) =
            ViewerHandshake::reconnect(&device, &agent.public(), LIMIT).unwrap();
        assert_eq!(format!("{handshake:?}"), "ViewerHandshake { .. }");
    }
}
