use crate::error::ProtocolError;

pub const LINK_ID_LEN: usize = 16;
pub const LINK_KEY_MIN: usize = 4;
pub const LINK_KEY_MAX: usize = 40;
pub const LINK_VERIFY_LEN: usize = 4;
pub const LINK_CODE_LEN: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFailure {
    WrongCode,
    TooManyAttempts,
    Gone,
    Unavailable,
}

impl LinkFailure {
    fn tag(self) -> u8 {
        match self {
            LinkFailure::WrongCode => 0,
            LinkFailure::TooManyAttempts => 1,
            LinkFailure::Gone => 2,
            LinkFailure::Unavailable => 3,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(LinkFailure::WrongCode),
            1 => Ok(LinkFailure::TooManyAttempts),
            2 => Ok(LinkFailure::Gone),
            3 => Ok(LinkFailure::Unavailable),
            other => Err(ProtocolError::InvalidControl(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkMessage {
    Lookup(String),
    Info {
        id: [u8; LINK_ID_LEN],
        verification: String,
    },
    NotFound,
    Approve {
        id: [u8; LINK_ID_LEN],
        code: String,
    },
    Approved,
    Failed(LinkFailure),
}

const LOOKUP: u8 = 0;
const INFO: u8 = 1;
const NOT_FOUND: u8 = 2;
const APPROVE: u8 = 3;
const APPROVED: u8 = 4;
const FAILED: u8 = 5;

fn digits(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_digit())
}

impl LinkMessage {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            LinkMessage::Lookup(key) => {
                let mut out = vec![LOOKUP];
                out.extend_from_slice(key.as_bytes());
                out
            }
            LinkMessage::Info { id, verification } => {
                let mut out = vec![INFO];
                out.extend_from_slice(id);
                out.extend_from_slice(verification.as_bytes());
                out
            }
            LinkMessage::NotFound => vec![NOT_FOUND],
            LinkMessage::Approve { id, code } => {
                let mut out = vec![APPROVE];
                out.extend_from_slice(id);
                out.extend_from_slice(code.as_bytes());
                out
            }
            LinkMessage::Approved => vec![APPROVED],
            LinkMessage::Failed(reason) => vec![FAILED, reason.tag()],
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&tag, body) = bytes
            .split_first()
            .ok_or(ProtocolError::Truncated { len: 0, needed: 1 })?;
        match tag {
            LOOKUP => {
                if !(LINK_KEY_MIN..=LINK_KEY_MAX).contains(&body.len()) {
                    return Err(bad_length(tag, LINK_KEY_MAX, body.len()));
                }
                let text =
                    std::str::from_utf8(body).map_err(|_| ProtocolError::InvalidText(tag))?;
                if !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    return Err(ProtocolError::InvalidText(tag));
                }
                Ok(LinkMessage::Lookup(text.to_owned()))
            }
            INFO => {
                if body.len() != LINK_ID_LEN + LINK_VERIFY_LEN {
                    return Err(bad_length(tag, LINK_ID_LEN + LINK_VERIFY_LEN, body.len()));
                }
                let (id, rest) = body.split_at(LINK_ID_LEN);
                let verification =
                    std::str::from_utf8(rest).map_err(|_| ProtocolError::InvalidText(tag))?;
                if !digits(verification, LINK_VERIFY_LEN) {
                    return Err(ProtocolError::InvalidText(tag));
                }
                Ok(LinkMessage::Info {
                    id: id
                        .try_into()
                        .map_err(|_| bad_length(tag, LINK_ID_LEN, id.len()))?,
                    verification: verification.to_owned(),
                })
            }
            APPROVE => {
                if body.len() != LINK_ID_LEN + LINK_CODE_LEN {
                    return Err(bad_length(tag, LINK_ID_LEN + LINK_CODE_LEN, body.len()));
                }
                let (id, rest) = body.split_at(LINK_ID_LEN);
                let code =
                    std::str::from_utf8(rest).map_err(|_| ProtocolError::InvalidText(tag))?;
                if !digits(code, LINK_CODE_LEN) {
                    return Err(ProtocolError::InvalidText(tag));
                }
                Ok(LinkMessage::Approve {
                    id: id
                        .try_into()
                        .map_err(|_| bad_length(tag, LINK_ID_LEN, id.len()))?,
                    code: code.to_owned(),
                })
            }
            NOT_FOUND | APPROVED => {
                if !body.is_empty() {
                    return Err(bad_length(tag, 0, body.len()));
                }
                Ok(if tag == NOT_FOUND {
                    LinkMessage::NotFound
                } else {
                    LinkMessage::Approved
                })
            }
            FAILED => match body {
                [reason] => Ok(LinkMessage::Failed(LinkFailure::from_tag(*reason)?)),
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

    fn samples() -> Vec<LinkMessage> {
        vec![
            LinkMessage::Lookup("ABCD-EFGH".to_owned()),
            LinkMessage::Info {
                id: [7; LINK_ID_LEN],
                verification: "0042".to_owned(),
            },
            LinkMessage::NotFound,
            LinkMessage::Approve {
                id: [9; LINK_ID_LEN],
                code: "123456".to_owned(),
            },
            LinkMessage::Approved,
            LinkMessage::Failed(LinkFailure::WrongCode),
            LinkMessage::Failed(LinkFailure::TooManyAttempts),
            LinkMessage::Failed(LinkFailure::Gone),
            LinkMessage::Failed(LinkFailure::Unavailable),
        ]
    }

    #[test]
    fn round_trips_every_message() {
        for message in samples() {
            assert_eq!(LinkMessage::decode(&message.encode()).unwrap(), message);
        }
    }

    #[test]
    fn rejects_malformed_messages() {
        assert!(LinkMessage::decode(&[]).is_err());
        assert!(LinkMessage::decode(&[99]).is_err());
        assert!(LinkMessage::decode(&[LOOKUP, b'a']).is_err());
        assert!(LinkMessage::decode(&[LOOKUP; 60]).is_err());
        assert!(LinkMessage::decode(b"\x00AB CD").is_err());
        assert!(LinkMessage::decode(&[NOT_FOUND, 0]).is_err());
        assert!(LinkMessage::decode(&[FAILED]).is_err());
        assert!(LinkMessage::decode(&[FAILED, 77]).is_err());
        let mut info = vec![INFO];
        info.extend_from_slice(&[0; LINK_ID_LEN]);
        info.extend_from_slice(b"12ab");
        assert!(LinkMessage::decode(&info).is_err());
        let mut approve = vec![APPROVE];
        approve.extend_from_slice(&[0; LINK_ID_LEN]);
        approve.extend_from_slice(b"12345");
        assert!(LinkMessage::decode(&approve).is_err());
    }
}
