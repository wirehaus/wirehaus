use crate::error::ProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Jpeg,
}

impl Codec {
    fn tag(self) -> u8 {
        match self {
            Codec::Jpeg => 0,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(Codec::Jpeg),
            other => Err(ProtocolError::UnknownCodec(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopMessage {
    Resize {
        width: u16,
        height: u16,
    },
    Tile {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        codec: Codec,
        data: Vec<u8>,
    },
    FrameEnd {
        sequence: u32,
    },
}

const RESIZE: u8 = 0;
const TILE: u8 = 1;
const FRAME_END: u8 = 2;
const TILE_HEADER_LEN: usize = 9;

impl DesktopMessage {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            DesktopMessage::Resize { width, height } => {
                let mut out = vec![RESIZE];
                out.extend_from_slice(&width.to_be_bytes());
                out.extend_from_slice(&height.to_be_bytes());
                out
            }
            DesktopMessage::Tile {
                x,
                y,
                width,
                height,
                codec,
                data,
            } => {
                let mut out = Vec::with_capacity(1 + TILE_HEADER_LEN + data.len());
                out.push(TILE);
                for value in [x, y, width, height] {
                    out.extend_from_slice(&value.to_be_bytes());
                }
                out.push(codec.tag());
                out.extend_from_slice(data);
                out
            }
            DesktopMessage::FrameEnd { sequence } => {
                let mut out = vec![FRAME_END];
                out.extend_from_slice(&sequence.to_be_bytes());
                out
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&tag, body) = bytes
            .split_first()
            .ok_or(ProtocolError::Truncated { len: 0, needed: 1 })?;
        match tag {
            RESIZE => {
                let [w0, w1, h0, h1]: [u8; 4] = body
                    .try_into()
                    .map_err(|_| bad_length(tag, 4, body.len()))?;
                Ok(DesktopMessage::Resize {
                    width: u16::from_be_bytes([w0, w1]),
                    height: u16::from_be_bytes([h0, h1]),
                })
            }
            TILE => {
                let (header, data) = body
                    .split_at_checked(TILE_HEADER_LEN)
                    .ok_or_else(|| bad_length(tag, TILE_HEADER_LEN, body.len()))?;
                let (data_header, codec) = header.split_at(8);
                let value = |i: usize| u16::from_be_bytes([data_header[i], data_header[i + 1]]);
                Ok(DesktopMessage::Tile {
                    x: value(0),
                    y: value(2),
                    width: value(4),
                    height: value(6),
                    codec: Codec::from_tag(codec[0])?,
                    data: data.to_vec(),
                })
            }
            FRAME_END => {
                let bytes: [u8; 4] = body
                    .try_into()
                    .map_err(|_| bad_length(tag, 4, body.len()))?;
                Ok(DesktopMessage::FrameEnd {
                    sequence: u32::from_be_bytes(bytes),
                })
            }
            other => Err(ProtocolError::UnknownDesktop(other)),
        }
    }
}

fn bad_length(tag: u8, expected: usize, actual: usize) -> ProtocolError {
    ProtocolError::BadDesktopLength {
        tag,
        expected,
        actual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile() -> DesktopMessage {
        DesktopMessage::Tile {
            x: 64,
            y: 128,
            width: 256,
            height: 64,
            codec: Codec::Jpeg,
            data: vec![0xff, 0xd8, 0xff],
        }
    }

    #[test]
    fn round_trips_every_message() {
        for message in [
            DesktopMessage::Resize {
                width: 1920,
                height: 1080,
            },
            tile(),
        ] {
            assert_eq!(DesktopMessage::decode(&message.encode()).unwrap(), message);
        }
    }

    #[test]
    fn encodes_a_known_layout() {
        assert_eq!(
            DesktopMessage::Resize {
                width: 0x0102,
                height: 0x0304
            }
            .encode(),
            [0, 1, 2, 3, 4]
        );
        assert_eq!(
            tile().encode(),
            [1, 0, 64, 0, 128, 1, 0, 0, 64, 0, 0xff, 0xd8, 0xff]
        );
    }

    #[test]
    fn encodes_the_frame_end_layout() {
        assert_eq!(
            DesktopMessage::FrameEnd {
                sequence: 0x0102_0304
            }
            .encode(),
            [2, 1, 2, 3, 4]
        );
    }

    #[test]
    fn rejects_a_frame_end_of_the_wrong_length() {
        assert!(matches!(
            DesktopMessage::decode(&[2, 1, 2, 3]),
            Err(ProtocolError::BadDesktopLength { tag: 2, .. })
        ));
        assert!(matches!(
            DesktopMessage::decode(&[2, 1, 2, 3, 4, 5]),
            Err(ProtocolError::BadDesktopLength { tag: 2, .. })
        ));
    }

    #[test]
    fn accepts_a_tile_without_data() {
        let message = DesktopMessage::Tile {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            codec: Codec::Jpeg,
            data: vec![],
        };
        assert_eq!(DesktopMessage::decode(&message.encode()).unwrap(), message);
    }

    #[test]
    fn rejects_an_empty_payload() {
        assert_eq!(
            DesktopMessage::decode(&[]),
            Err(ProtocolError::Truncated { len: 0, needed: 1 })
        );
    }

    #[test]
    fn rejects_an_unknown_message() {
        assert_eq!(
            DesktopMessage::decode(&[9]),
            Err(ProtocolError::UnknownDesktop(9))
        );
    }

    #[test]
    fn rejects_a_resize_of_the_wrong_length() {
        assert!(matches!(
            DesktopMessage::decode(&[0, 1, 2, 3]),
            Err(ProtocolError::BadDesktopLength { tag: 0, .. })
        ));
        assert!(matches!(
            DesktopMessage::decode(&[0, 1, 2, 3, 4, 5]),
            Err(ProtocolError::BadDesktopLength { tag: 0, .. })
        ));
    }

    #[test]
    fn rejects_a_short_tile_header() {
        assert!(matches!(
            DesktopMessage::decode(&[1, 0, 0, 0, 0, 0, 1, 0, 1]),
            Err(ProtocolError::BadDesktopLength { tag: 1, .. })
        ));
    }

    #[test]
    fn rejects_an_unknown_codec() {
        let mut bytes = tile().encode();
        bytes[9] = 77;
        assert_eq!(
            DesktopMessage::decode(&bytes),
            Err(ProtocolError::UnknownCodec(77))
        );
    }
}
