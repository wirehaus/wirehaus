use crate::error::ProtocolError;

pub const MAX_AUDIO_PACKET: usize = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFailure {
    NotAllowed,
    Unavailable,
}

impl AudioFailure {
    fn tag(self) -> u8 {
        match self {
            AudioFailure::NotAllowed => 0,
            AudioFailure::Unavailable => 1,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(AudioFailure::NotAllowed),
            1 => Ok(AudioFailure::Unavailable),
            other => Err(ProtocolError::InvalidControl(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioMessage {
    Start,
    Stop,
    Started,
    Stopped,
    Packet(Vec<u8>),
    Failed(AudioFailure),
}

const START: u8 = 0;
const STOP: u8 = 1;
const STARTED: u8 = 2;
const STOPPED: u8 = 3;
const PACKET: u8 = 4;
const FAILED: u8 = 5;

impl AudioMessage {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            AudioMessage::Start => vec![START],
            AudioMessage::Stop => vec![STOP],
            AudioMessage::Started => vec![STARTED],
            AudioMessage::Stopped => vec![STOPPED],
            AudioMessage::Packet(data) => {
                let mut out = Vec::with_capacity(1 + data.len());
                out.push(PACKET);
                out.extend_from_slice(data);
                out
            }
            AudioMessage::Failed(reason) => vec![FAILED, reason.tag()],
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&tag, body) = bytes
            .split_first()
            .ok_or(ProtocolError::Truncated { len: 0, needed: 1 })?;
        match tag {
            START | STOP | STARTED | STOPPED => {
                if !body.is_empty() {
                    return Err(bad_length(tag, 0, body.len()));
                }
                Ok(match tag {
                    START => AudioMessage::Start,
                    STOP => AudioMessage::Stop,
                    STARTED => AudioMessage::Started,
                    _ => AudioMessage::Stopped,
                })
            }
            PACKET => {
                if body.is_empty() || body.len() > MAX_AUDIO_PACKET {
                    return Err(bad_length(tag, MAX_AUDIO_PACKET, body.len()));
                }
                Ok(AudioMessage::Packet(body.to_vec()))
            }
            FAILED => match body {
                [reason] => Ok(AudioMessage::Failed(AudioFailure::from_tag(*reason)?)),
                _ => Err(bad_length(tag, 1, body.len())),
            },
            other => Err(ProtocolError::UnknownControl(other)),
        }
    }
}

fn bad_length(tag: u8, expected: usize, actual: usize) -> ProtocolError {
    ProtocolError::BadControlLength {
        tag,
        expected,
        actual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<AudioMessage> {
        vec![
            AudioMessage::Start,
            AudioMessage::Stop,
            AudioMessage::Started,
            AudioMessage::Stopped,
            AudioMessage::Packet(vec![1, 2, 3]),
            AudioMessage::Failed(AudioFailure::NotAllowed),
            AudioMessage::Failed(AudioFailure::Unavailable),
        ]
    }

    #[test]
    fn round_trips_every_message() {
        for message in samples() {
            assert_eq!(AudioMessage::decode(&message.encode()).unwrap(), message);
        }
    }

    #[test]
    fn encodes_known_layouts() {
        assert_eq!(AudioMessage::Start.encode(), [0]);
        assert_eq!(AudioMessage::Stop.encode(), [1]);
        assert_eq!(AudioMessage::Started.encode(), [2]);
        assert_eq!(AudioMessage::Stopped.encode(), [3]);
        assert_eq!(AudioMessage::Packet(vec![9, 8]).encode(), [4, 9, 8]);
        assert_eq!(
            AudioMessage::Failed(AudioFailure::Unavailable).encode(),
            [5, 1]
        );
    }

    #[test]
    fn packets_are_never_empty_and_are_limited() {
        assert!(AudioMessage::decode(&[4]).is_err());
        let mut packet = vec![4];
        packet.resize(1 + MAX_AUDIO_PACKET, 0);
        assert!(AudioMessage::decode(&packet).is_ok());
        packet.push(0);
        assert!(AudioMessage::decode(&packet).is_err());
    }

    #[test]
    fn rejects_malformed_messages() {
        for bytes in [
            &[][..],
            &[6],
            &[0, 0],
            &[1, 0],
            &[2, 0],
            &[3, 0],
            &[5],
            &[5, 2],
            &[5, 0, 0],
        ] {
            assert!(AudioMessage::decode(bytes).is_err(), "{bytes:?}");
        }
    }
}
