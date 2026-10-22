use crate::error::ProtocolError;

pub const CODE_LEN: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailure {
    WrongCode,
    Locked,
    Suspended,
    Busy,
}

impl AuthFailure {
    fn tag(self) -> u8 {
        match self {
            AuthFailure::WrongCode => 0,
            AuthFailure::Locked => 1,
            AuthFailure::Suspended => 2,
            AuthFailure::Busy => 3,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(AuthFailure::WrongCode),
            1 => Ok(AuthFailure::Locked),
            2 => Ok(AuthFailure::Suspended),
            3 => Ok(AuthFailure::Busy),
            other => Err(ProtocolError::InvalidControl(other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlMessage {
    AuthRequired,
    Totp {
        code: [u8; CODE_LEN],
    },
    AuthOk,
    AuthFailed {
        reason: AuthFailure,
        retry_after: u32,
    },
}

const AUTH_REQUIRED: u8 = 0;
const TOTP: u8 = 1;
const AUTH_OK: u8 = 2;
const AUTH_FAILED: u8 = 3;

impl ControlMessage {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            ControlMessage::AuthRequired => vec![AUTH_REQUIRED],
            ControlMessage::Totp { code } => {
                let mut out = vec![TOTP];
                out.extend_from_slice(code);
                out
            }
            ControlMessage::AuthOk => vec![AUTH_OK],
            ControlMessage::AuthFailed {
                reason,
                retry_after,
            } => {
                let mut out = vec![AUTH_FAILED, reason.tag()];
                out.extend_from_slice(&retry_after.to_be_bytes());
                out
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&tag, body) = bytes
            .split_first()
            .ok_or(ProtocolError::Truncated { len: 0, needed: 1 })?;
        match tag {
            AUTH_REQUIRED => empty(tag, body).map(|()| ControlMessage::AuthRequired),
            AUTH_OK => empty(tag, body).map(|()| ControlMessage::AuthOk),
            TOTP => {
                let code: [u8; CODE_LEN] = body
                    .try_into()
                    .map_err(|_| bad_length(tag, CODE_LEN, body.len()))?;
                if let Some(&bad) = code.iter().find(|b| !b.is_ascii_digit()) {
                    return Err(ProtocolError::InvalidControl(bad));
                }
                Ok(ControlMessage::Totp { code })
            }
            AUTH_FAILED => {
                let [reason, a, b, c, d]: [u8; 5] = body
                    .try_into()
                    .map_err(|_| bad_length(tag, 5, body.len()))?;
                Ok(ControlMessage::AuthFailed {
                    reason: AuthFailure::from_tag(reason)?,
                    retry_after: u32::from_be_bytes([a, b, c, d]),
                })
            }
            other => Err(ProtocolError::UnknownControl(other)),
        }
    }
}

fn empty(tag: u8, body: &[u8]) -> Result<(), ProtocolError> {
    if body.is_empty() {
        Ok(())
    } else {
        Err(bad_length(tag, 0, body.len()))
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

    const SAMPLES: [ControlMessage; 6] = [
        ControlMessage::AuthRequired,
        ControlMessage::AuthOk,
        ControlMessage::Totp { code: *b"012345" },
        ControlMessage::AuthFailed {
            reason: AuthFailure::WrongCode,
            retry_after: 0,
        },
        ControlMessage::AuthFailed {
            reason: AuthFailure::Locked,
            retry_after: 3600,
        },
        ControlMessage::AuthFailed {
            reason: AuthFailure::Busy,
            retry_after: u32::MAX,
        },
    ];

    #[test]
    fn round_trips_every_message() {
        for message in SAMPLES {
            assert_eq!(ControlMessage::decode(&message.encode()).unwrap(), message);
        }
        for reason in [AuthFailure::Suspended, AuthFailure::WrongCode] {
            let message = ControlMessage::AuthFailed {
                reason,
                retry_after: 7,
            };
            assert_eq!(ControlMessage::decode(&message.encode()).unwrap(), message);
        }
    }

    #[test]
    fn encodes_known_layouts() {
        assert_eq!(ControlMessage::AuthRequired.encode(), [0]);
        assert_eq!(ControlMessage::AuthOk.encode(), [2]);
        assert_eq!(
            ControlMessage::Totp { code: *b"123456" }.encode(),
            [1, b'1', b'2', b'3', b'4', b'5', b'6']
        );
        assert_eq!(
            ControlMessage::AuthFailed {
                reason: AuthFailure::Locked,
                retry_after: 0x0102_0304
            }
            .encode(),
            [3, 1, 1, 2, 3, 4]
        );
    }

    #[test]
    fn rejects_empty_and_unknown_messages() {
        assert_eq!(
            ControlMessage::decode(&[]),
            Err(ProtocolError::Truncated { len: 0, needed: 1 })
        );
        assert_eq!(
            ControlMessage::decode(&[9]),
            Err(ProtocolError::UnknownControl(9))
        );
    }

    #[test]
    fn rejects_wrong_lengths() {
        for bytes in [
            &[0, 1][..],
            &[2, 0],
            &[1, b'1'],
            &[1, b'1', b'2', b'3', b'4', b'5', b'6', b'7'],
            &[3, 0, 0, 0, 0],
        ] {
            assert!(
                matches!(
                    ControlMessage::decode(bytes),
                    Err(ProtocolError::BadControlLength { .. })
                ),
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn a_code_must_be_ascii_digits() {
        for code in [*b"12345a", *b" 12345", *b"12 456", [0xFF; 6], *b"-12345"] {
            let mut bytes = vec![1];
            bytes.extend_from_slice(&code);
            assert!(matches!(
                ControlMessage::decode(&bytes),
                Err(ProtocolError::InvalidControl(_))
            ));
        }
    }

    #[test]
    fn rejects_an_unknown_failure_reason() {
        assert_eq!(
            ControlMessage::decode(&[3, 9, 0, 0, 0, 0]),
            Err(ProtocolError::InvalidControl(9))
        );
    }
}
