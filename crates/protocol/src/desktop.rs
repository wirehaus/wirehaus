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
    Video {
        width: u16,
        height: u16,
        keyframe: bool,
        data: Vec<u8>,
    },
    Monitors {
        active: u8,
        items: Vec<MonitorInfo>,
    },
}

pub const MAX_MONITORS: usize = 16;
const MONITOR_LEN: usize = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorInfo {
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
    pub primary: bool,
    pub all: bool,
}

const RESIZE: u8 = 0;
const TILE: u8 = 1;
const FRAME_END: u8 = 2;
const VIDEO: u8 = 3;
const MONITORS: u8 = 4;
const VIDEO_HEADER_LEN: usize = 5;
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
            DesktopMessage::Video {
                width,
                height,
                keyframe,
                data,
            } => {
                let mut out = Vec::with_capacity(1 + VIDEO_HEADER_LEN + data.len());
                out.push(VIDEO);
                out.extend_from_slice(&width.to_be_bytes());
                out.extend_from_slice(&height.to_be_bytes());
                out.push(u8::from(*keyframe));
                out.extend_from_slice(data);
                out
            }
            DesktopMessage::Monitors { active, items } => {
                let mut out = Vec::with_capacity(3 + items.len() * MONITOR_LEN);
                out.push(MONITORS);
                out.push(*active);
                out.push(u8::try_from(items.len().min(MAX_MONITORS)).unwrap_or(0));
                for item in items.iter().take(MAX_MONITORS) {
                    out.extend_from_slice(&item.x.to_be_bytes());
                    out.extend_from_slice(&item.y.to_be_bytes());
                    out.extend_from_slice(&item.width.to_be_bytes());
                    out.extend_from_slice(&item.height.to_be_bytes());
                    out.push(u8::from(item.primary) | (u8::from(item.all) << 1));
                }
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
            VIDEO => {
                let (header, data) = body
                    .split_at_checked(VIDEO_HEADER_LEN)
                    .ok_or_else(|| bad_length(tag, VIDEO_HEADER_LEN, body.len()))?;
                let keyframe = match header[4] {
                    0 => false,
                    1 => true,
                    other => return Err(ProtocolError::InvalidControl(other)),
                };
                Ok(DesktopMessage::Video {
                    width: u16::from_be_bytes([header[0], header[1]]),
                    height: u16::from_be_bytes([header[2], header[3]]),
                    keyframe,
                    data: data.to_vec(),
                })
            }
            MONITORS => {
                let (&active, rest) = body.split_first().ok_or_else(|| bad_length(tag, 2, 0))?;
                let (&count, list) = rest.split_first().ok_or_else(|| bad_length(tag, 2, 1))?;
                let count = usize::from(count);
                if count > MAX_MONITORS || list.len() != count * MONITOR_LEN {
                    return Err(bad_length(tag, 2 + count * MONITOR_LEN, body.len()));
                }
                let items = list
                    .as_chunks::<MONITOR_LEN>()
                    .0
                    .iter()
                    .map(|chunk| MonitorInfo {
                        x: i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                        y: i32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
                        width: u16::from_be_bytes([chunk[8], chunk[9]]),
                        height: u16::from_be_bytes([chunk[10], chunk[11]]),
                        primary: chunk[12] & 1 != 0,
                        all: chunk[12] & 2 != 0,
                    })
                    .collect();
                Ok(DesktopMessage::Monitors { active, items })
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
            DesktopMessage::Monitors {
                active: 1,
                items: vec![
                    MonitorInfo {
                        x: -1920,
                        y: 0,
                        width: 1920,
                        height: 1080,
                        primary: false,
                        all: false,
                    },
                    MonitorInfo {
                        x: 0,
                        y: 0,
                        width: 3456,
                        height: 2234,
                        primary: true,
                        all: false,
                    },
                    MonitorInfo {
                        x: 0,
                        y: 0,
                        width: 5376,
                        height: 2234,
                        primary: false,
                        all: true,
                    },
                ],
            },
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
    fn encodes_the_video_layout() {
        assert_eq!(
            DesktopMessage::Video {
                width: 0x0102,
                height: 0x0304,
                keyframe: true,
                data: vec![9, 8]
            }
            .encode(),
            [3, 1, 2, 3, 4, 1, 9, 8]
        );
    }

    #[test]
    fn rejects_malformed_video_messages() {
        assert!(matches!(
            DesktopMessage::decode(&[3, 0, 8, 0, 8]),
            Err(ProtocolError::BadDesktopLength { tag: 3, .. })
        ));
        assert!(matches!(
            DesktopMessage::decode(&[3, 0, 8, 0, 8, 2]),
            Err(ProtocolError::InvalidControl(2))
        ));
        assert!(DesktopMessage::decode(&[3, 0, 8, 0, 8, 0]).is_ok());
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

#[cfg(test)]
mod monitor_tests {
    use super::*;

    #[test]
    fn a_monitor_list_with_the_wrong_length_or_too_many_entries_is_refused() {
        assert!(DesktopMessage::decode(&[MONITORS, 0]).is_err());
        assert!(DesktopMessage::decode(&[MONITORS, 0, 1, 0, 0]).is_err());
        let mut many = vec![MONITORS, 0, 17];
        many.extend(vec![0; 17 * MONITOR_LEN]);
        assert!(DesktopMessage::decode(&many).is_err());
        assert_eq!(
            DesktopMessage::decode(&[MONITORS, 0, 0]).unwrap(),
            DesktopMessage::Monitors {
                active: 0,
                items: Vec::new()
            }
        );
    }
}
