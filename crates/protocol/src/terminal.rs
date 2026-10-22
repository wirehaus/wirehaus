use crate::error::ProtocolError;

pub const MAX_INPUT_CHUNK: usize = 3072;
pub const MAX_OUTPUT_CHUNK: usize = 16 * 1024;
pub const MAX_DIMENSION: u16 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    NotAllowed,
    TooManySessions,
    SpawnFailed,
}

impl RefusalReason {
    fn tag(self) -> u8 {
        match self {
            RefusalReason::NotAllowed => 0,
            RefusalReason::TooManySessions => 1,
            RefusalReason::SpawnFailed => 2,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(RefusalReason::NotAllowed),
            1 => Ok(RefusalReason::TooManySessions),
            2 => Ok(RefusalReason::SpawnFailed),
            other => Err(ProtocolError::InvalidControl(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalMessage {
    Open { columns: u16, rows: u16 },
    Input(Vec<u8>),
    Resize { columns: u16, rows: u16 },
    Close,
    Output(Vec<u8>),
    Exited { code: i32 },
    Refused(RefusalReason),
}

const OPEN: u8 = 0;
const INPUT: u8 = 1;
const RESIZE: u8 = 2;
const CLOSE: u8 = 3;
const OUTPUT: u8 = 4;
const EXITED: u8 = 5;
const REFUSED: u8 = 6;

impl TerminalMessage {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            TerminalMessage::Open { columns, rows } => size_message(OPEN, *columns, *rows),
            TerminalMessage::Resize { columns, rows } => size_message(RESIZE, *columns, *rows),
            TerminalMessage::Input(data) => with_data(INPUT, data),
            TerminalMessage::Output(data) => with_data(OUTPUT, data),
            TerminalMessage::Close => vec![CLOSE],
            TerminalMessage::Exited { code } => {
                let mut out = vec![EXITED];
                out.extend_from_slice(&code.to_be_bytes());
                out
            }
            TerminalMessage::Refused(reason) => vec![REFUSED, reason.tag()],
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&tag, body) = bytes
            .split_first()
            .ok_or(ProtocolError::Truncated { len: 0, needed: 1 })?;
        match tag {
            OPEN | RESIZE => {
                let [c0, c1, r0, r1]: [u8; 4] = body
                    .try_into()
                    .map_err(|_| bad_length(tag, 4, body.len()))?;
                let (columns, rows) = (u16::from_be_bytes([c0, c1]), u16::from_be_bytes([r0, r1]));
                if !(1..=MAX_DIMENSION).contains(&columns) || !(1..=MAX_DIMENSION).contains(&rows) {
                    return Err(ProtocolError::InvalidControl(tag));
                }
                Ok(if tag == OPEN {
                    TerminalMessage::Open { columns, rows }
                } else {
                    TerminalMessage::Resize { columns, rows }
                })
            }
            INPUT | OUTPUT => {
                let limit = if tag == INPUT {
                    MAX_INPUT_CHUNK
                } else {
                    MAX_OUTPUT_CHUNK
                };
                if body.is_empty() || body.len() > limit {
                    return Err(bad_length(tag, limit, body.len()));
                }
                Ok(if tag == INPUT {
                    TerminalMessage::Input(body.to_vec())
                } else {
                    TerminalMessage::Output(body.to_vec())
                })
            }
            CLOSE => {
                if body.is_empty() {
                    Ok(TerminalMessage::Close)
                } else {
                    Err(bad_length(tag, 0, body.len()))
                }
            }
            EXITED => {
                let code: [u8; 4] = body
                    .try_into()
                    .map_err(|_| bad_length(tag, 4, body.len()))?;
                Ok(TerminalMessage::Exited {
                    code: i32::from_be_bytes(code),
                })
            }
            REFUSED => match body {
                [reason] => Ok(TerminalMessage::Refused(RefusalReason::from_tag(*reason)?)),
                _ => Err(bad_length(tag, 1, body.len())),
            },
            other => Err(ProtocolError::UnknownControl(other)),
        }
    }
}

fn size_message(tag: u8, columns: u16, rows: u16) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend_from_slice(&columns.to_be_bytes());
    out.extend_from_slice(&rows.to_be_bytes());
    out
}

fn with_data(tag: u8, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + data.len());
    out.push(tag);
    out.extend_from_slice(data);
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

    fn samples() -> Vec<TerminalMessage> {
        vec![
            TerminalMessage::Open {
                columns: 80,
                rows: 24,
            },
            TerminalMessage::Resize {
                columns: 500,
                rows: 1,
            },
            TerminalMessage::Input(b"ls -la\n".to_vec()),
            TerminalMessage::Output(vec![0x1b, b'[', b'3', b'1', b'm', 0xC3, 0xB1]),
            TerminalMessage::Close,
            TerminalMessage::Exited { code: -1 },
            TerminalMessage::Exited { code: 130 },
            TerminalMessage::Refused(RefusalReason::NotAllowed),
            TerminalMessage::Refused(RefusalReason::TooManySessions),
            TerminalMessage::Refused(RefusalReason::SpawnFailed),
        ]
    }

    #[test]
    fn round_trips_every_message() {
        for message in samples() {
            assert_eq!(TerminalMessage::decode(&message.encode()).unwrap(), message);
        }
    }

    #[test]
    fn encodes_known_layouts() {
        assert_eq!(
            TerminalMessage::Open {
                columns: 0x0102,
                rows: 0x0034
            }
            .encode(),
            [0, 1, 2, 0, 0x34]
        );
        assert_eq!(TerminalMessage::Input(vec![65]).encode(), [1, 65]);
        assert_eq!(TerminalMessage::Close.encode(), [3]);
        assert_eq!(
            TerminalMessage::Exited { code: 1 }.encode(),
            [5, 0, 0, 0, 1]
        );
        assert_eq!(
            TerminalMessage::Refused(RefusalReason::TooManySessions).encode(),
            [6, 1]
        );
    }

    #[test]
    fn rejects_bad_dimensions() {
        for (columns, rows) in [(0, 24), (80, 0), (501, 24), (80, 501), (u16::MAX, 1)] {
            let bytes = TerminalMessage::Open { columns, rows }.encode();
            assert!(TerminalMessage::decode(&bytes).is_err(), "{columns}x{rows}");
        }
    }

    #[test]
    fn rejects_empty_and_oversized_data() {
        assert!(TerminalMessage::decode(&[1]).is_err());
        assert!(TerminalMessage::decode(&[4]).is_err());
        let mut input = vec![1];
        input.resize(1 + MAX_INPUT_CHUNK + 1, 0);
        assert!(TerminalMessage::decode(&input).is_err());
        input.truncate(1 + MAX_INPUT_CHUNK);
        assert!(TerminalMessage::decode(&input).is_ok());
        let mut output = vec![4];
        output.resize(1 + MAX_OUTPUT_CHUNK + 1, 0);
        assert!(TerminalMessage::decode(&output).is_err());
    }

    #[test]
    fn rejects_malformed_messages() {
        assert!(TerminalMessage::decode(&[]).is_err());
        assert!(TerminalMessage::decode(&[9]).is_err());
        assert!(TerminalMessage::decode(&[3, 0]).is_err());
        assert!(TerminalMessage::decode(&[5, 0, 0, 1]).is_err());
        assert!(TerminalMessage::decode(&[6]).is_err());
        assert!(TerminalMessage::decode(&[6, 9]).is_err());
        assert!(TerminalMessage::decode(&[0, 0, 80, 0]).is_err());
        assert!(TerminalMessage::decode(&[2, 0, 80, 0, 24, 0]).is_err());
    }
}
