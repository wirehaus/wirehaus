use crate::error::ProtocolError;

pub const MAX_CLIPBOARD_TEXT: usize = 32 * 1024;

const _: () = assert!(crate::HEADER_LEN + 1 + MAX_CLIPBOARD_TEXT <= crate::MAX_VIEWER_MESSAGE);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardFailure {
    NotAllowed,
    TooLarge,
    Unavailable,
    Timeout,
}

impl ClipboardFailure {
    fn tag(self) -> u8 {
        match self {
            ClipboardFailure::NotAllowed => 0,
            ClipboardFailure::TooLarge => 1,
            ClipboardFailure::Unavailable => 2,
            ClipboardFailure::Timeout => 3,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(ClipboardFailure::NotAllowed),
            1 => Ok(ClipboardFailure::TooLarge),
            2 => Ok(ClipboardFailure::Unavailable),
            3 => Ok(ClipboardFailure::Timeout),
            other => Err(ProtocolError::InvalidControl(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardMessage {
    Write(String),
    Read,
    Content(String),
    Empty,
    Written,
    Failed(ClipboardFailure),
}

const WRITE: u8 = 0;
const READ: u8 = 1;
const CONTENT: u8 = 2;
const EMPTY: u8 = 3;
const WRITTEN: u8 = 4;
const FAILED: u8 = 5;

impl ClipboardMessage {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            ClipboardMessage::Write(text) => with_text(WRITE, text),
            ClipboardMessage::Content(text) => with_text(CONTENT, text),
            ClipboardMessage::Read => vec![READ],
            ClipboardMessage::Empty => vec![EMPTY],
            ClipboardMessage::Written => vec![WRITTEN],
            ClipboardMessage::Failed(reason) => vec![FAILED, reason.tag()],
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&tag, body) = bytes
            .split_first()
            .ok_or(ProtocolError::Truncated { len: 0, needed: 1 })?;
        match tag {
            WRITE | CONTENT => {
                if body.is_empty() || body.len() > MAX_CLIPBOARD_TEXT {
                    return Err(bad_length(tag, MAX_CLIPBOARD_TEXT, body.len()));
                }
                let text = String::from_utf8(body.to_vec())
                    .map_err(|_| ProtocolError::InvalidText(tag))?;
                Ok(if tag == WRITE {
                    ClipboardMessage::Write(text)
                } else {
                    ClipboardMessage::Content(text)
                })
            }
            READ | EMPTY | WRITTEN => {
                if !body.is_empty() {
                    return Err(bad_length(tag, 0, body.len()));
                }
                Ok(match tag {
                    READ => ClipboardMessage::Read,
                    EMPTY => ClipboardMessage::Empty,
                    _ => ClipboardMessage::Written,
                })
            }
            FAILED => match body {
                [reason] => Ok(ClipboardMessage::Failed(ClipboardFailure::from_tag(
                    *reason,
                )?)),
                _ => Err(bad_length(tag, 1, body.len())),
            },
            other => Err(ProtocolError::UnknownControl(other)),
        }
    }
}

fn with_text(tag: u8, text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + text.len());
    out.push(tag);
    out.extend_from_slice(text.as_bytes());
    out
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

    fn samples() -> Vec<ClipboardMessage> {
        vec![
            ClipboardMessage::Write("hola ñandú".to_owned()),
            ClipboardMessage::Read,
            ClipboardMessage::Content("a\nb\tc".to_owned()),
            ClipboardMessage::Empty,
            ClipboardMessage::Written,
            ClipboardMessage::Failed(ClipboardFailure::NotAllowed),
            ClipboardMessage::Failed(ClipboardFailure::TooLarge),
            ClipboardMessage::Failed(ClipboardFailure::Unavailable),
            ClipboardMessage::Failed(ClipboardFailure::Timeout),
        ]
    }

    #[test]
    fn round_trips_every_message() {
        for message in samples() {
            assert_eq!(
                ClipboardMessage::decode(&message.encode()).unwrap(),
                message
            );
        }
    }

    #[test]
    fn encodes_known_layouts() {
        assert_eq!(
            ClipboardMessage::Write("ab".to_owned()).encode(),
            [0, 97, 98]
        );
        assert_eq!(ClipboardMessage::Read.encode(), [1]);
        assert_eq!(ClipboardMessage::Content("a".to_owned()).encode(), [2, 97]);
        assert_eq!(ClipboardMessage::Empty.encode(), [3]);
        assert_eq!(ClipboardMessage::Written.encode(), [4]);
        assert_eq!(
            ClipboardMessage::Failed(ClipboardFailure::Timeout).encode(),
            [5, 3]
        );
    }

    #[test]
    fn text_is_limited_and_must_be_utf8() {
        assert!(ClipboardMessage::decode(&[0]).is_err());
        assert!(ClipboardMessage::decode(&[2]).is_err());
        assert!(ClipboardMessage::decode(&[0, 0xFF, 0xFE]).is_err());
        let mut text = vec![0];
        text.resize(1 + MAX_CLIPBOARD_TEXT, b'a');
        assert!(ClipboardMessage::decode(&text).is_ok());
        text.push(b'a');
        assert!(ClipboardMessage::decode(&text).is_err());
    }

    #[test]
    fn rejects_malformed_messages() {
        for bytes in [
            &[][..],
            &[6],
            &[1, 0],
            &[3, 0],
            &[4, 1],
            &[5],
            &[5, 4],
            &[5, 0, 0],
        ] {
            assert!(ClipboardMessage::decode(bytes).is_err(), "{bytes:?}");
        }
    }
}
