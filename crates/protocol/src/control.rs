use crate::error::ProtocolError;

pub const CODE_LEN: usize = 6;
pub const TICKET_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailure {
    WrongCode,
    Locked,
    Suspended,
    Busy,
    Unavailable,
}

impl AuthFailure {
    fn tag(self) -> u8 {
        match self {
            AuthFailure::WrongCode => 0,
            AuthFailure::Locked => 1,
            AuthFailure::Suspended => 2,
            AuthFailure::Busy => 3,
            AuthFailure::Unavailable => 4,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(AuthFailure::WrongCode),
            1 => Ok(AuthFailure::Locked),
            2 => Ok(AuthFailure::Suspended),
            3 => Ok(AuthFailure::Busy),
            4 => Ok(AuthFailure::Unavailable),
            other => Err(ProtocolError::InvalidControl(other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Idle,
    Revoked,
    Locked,
}

impl EndReason {
    fn tag(self) -> u8 {
        match self {
            EndReason::Idle => 0,
            EndReason::Revoked => 1,
            EndReason::Locked => 2,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(EndReason::Idle),
            1 => Ok(EndReason::Revoked),
            2 => Ok(EndReason::Locked),
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
    Resume {
        ticket: [u8; TICKET_LEN],
    },
    Ticket {
        ticket: [u8; TICKET_LEN],
    },
    SessionEnded {
        reason: EndReason,
    },
}

const AUTH_REQUIRED: u8 = 0;
const TOTP: u8 = 1;
const AUTH_OK: u8 = 2;
const AUTH_FAILED: u8 = 3;
const RESUME: u8 = 4;
const TICKET: u8 = 5;
const SESSION_ENDED: u8 = 6;

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
            ControlMessage::Resume { ticket } => with_ticket(RESUME, ticket),
            ControlMessage::Ticket { ticket } => with_ticket(TICKET, ticket),
            ControlMessage::SessionEnded { reason } => vec![SESSION_ENDED, reason.tag()],
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
            RESUME | TICKET => {
                let ticket: [u8; TICKET_LEN] = body
                    .try_into()
                    .map_err(|_| bad_length(tag, TICKET_LEN, body.len()))?;
                Ok(if tag == RESUME {
                    ControlMessage::Resume { ticket }
                } else {
                    ControlMessage::Ticket { ticket }
                })
            }
            SESSION_ENDED => match body {
                [reason] => Ok(ControlMessage::SessionEnded {
                    reason: EndReason::from_tag(*reason)?,
                }),
                _ => Err(bad_length(tag, 1, body.len())),
            },
            other => Err(ProtocolError::UnknownControl(other)),
        }
    }
}

fn with_ticket(tag: u8, ticket: &[u8; TICKET_LEN]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend_from_slice(ticket);
    out
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

    const SAMPLES: [ControlMessage; 11] = [
        ControlMessage::Resume {
            ticket: [7; TICKET_LEN],
        },
        ControlMessage::Ticket {
            ticket: [0xAB; TICKET_LEN],
        },
        ControlMessage::SessionEnded {
            reason: EndReason::Idle,
        },
        ControlMessage::SessionEnded {
            reason: EndReason::Revoked,
        },
        ControlMessage::SessionEnded {
            reason: EndReason::Locked,
        },
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
        for reason in [
            AuthFailure::Suspended,
            AuthFailure::WrongCode,
            AuthFailure::Unavailable,
        ] {
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
    fn encodes_the_unavailable_refusal_and_rejects_the_next_unknown_reason() {
        assert_eq!(
            ControlMessage::AuthFailed {
                reason: AuthFailure::Unavailable,
                retry_after: 0
            }
            .encode(),
            [3, 4, 0, 0, 0, 0]
        );
        assert!(ControlMessage::decode(&[3, 5, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn encodes_the_resumption_messages_with_known_layouts() {
        let mut resume = vec![4];
        resume.extend_from_slice(&[9; TICKET_LEN]);
        assert_eq!(
            ControlMessage::Resume {
                ticket: [9; TICKET_LEN]
            }
            .encode(),
            resume
        );
        resume[0] = 5;
        assert_eq!(
            ControlMessage::Ticket {
                ticket: [9; TICKET_LEN]
            }
            .encode(),
            resume
        );
        assert_eq!(
            ControlMessage::SessionEnded {
                reason: EndReason::Locked
            }
            .encode(),
            [6, 2]
        );
    }

    #[test]
    fn rejects_malformed_resumption_messages() {
        for tag in [4_u8, 5] {
            for length in [0, 1, TICKET_LEN - 1, TICKET_LEN + 1] {
                let mut bytes = vec![tag];
                bytes.resize(1 + length, 1);
                assert!(
                    matches!(
                        ControlMessage::decode(&bytes),
                        Err(ProtocolError::BadControlLength { .. })
                    ),
                    "{tag} {length}"
                );
            }
        }
        assert!(ControlMessage::decode(&[6]).is_err());
        assert!(ControlMessage::decode(&[6, 0, 0]).is_err());
        assert_eq!(
            ControlMessage::decode(&[6, 9]),
            Err(ProtocolError::InvalidControl(9))
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
