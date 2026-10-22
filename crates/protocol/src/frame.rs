use crate::channel::Channel;
use crate::error::ProtocolError;

pub const HEADER_LEN: usize = 9;
pub const MAX_PAYLOAD_LEN: usize = 1 << 20;
pub const MAX_VIEWER_MESSAGE: usize = 36 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub channel: Channel,
    pub stream: u32,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(channel: Channel, stream: u32, payload: Vec<u8>) -> Result<Self, ProtocolError> {
        check_payload(payload.len())?;
        Ok(Self {
            channel,
            stream,
            payload,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        check_payload(self.payload.len())?;
        let declared =
            u32::try_from(self.payload.len()).map_err(|_| too_large(self.payload.len()))?;
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len());
        out.push(self.channel.tag());
        out.extend_from_slice(&self.stream.to_be_bytes());
        out.extend_from_slice(&declared.to_be_bytes());
        out.extend_from_slice(&self.payload);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (header, payload) =
            bytes
                .split_at_checked(HEADER_LEN)
                .ok_or(ProtocolError::Truncated {
                    len: bytes.len(),
                    needed: HEADER_LEN,
                })?;
        let channel = Channel::from_tag(header[0])?;
        let stream = u32::from_be_bytes([header[1], header[2], header[3], header[4]]);
        let declared = u32::from_be_bytes([header[5], header[6], header[7], header[8]]);
        let declared = usize::try_from(declared).map_err(|_| too_large(usize::MAX))?;
        check_payload(declared)?;
        if payload.len() != declared {
            return Err(ProtocolError::LengthMismatch {
                declared,
                actual: payload.len(),
            });
        }
        Ok(Self {
            channel,
            stream,
            payload: payload.to_vec(),
        })
    }
}

fn check_payload(len: usize) -> Result<(), ProtocolError> {
    if len > MAX_PAYLOAD_LEN {
        return Err(too_large(len));
    }
    Ok(())
}

fn too_large(len: usize) -> ProtocolError {
    ProtocolError::PayloadTooLarge {
        len,
        max: MAX_PAYLOAD_LEN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_channel() {
        for channel in Channel::ALL {
            let frame = Frame::new(channel, 7, vec![1, 2, 3]).unwrap();
            assert_eq!(Frame::decode(&frame.encode().unwrap()).unwrap(), frame);
        }
    }

    #[test]
    fn encodes_a_known_layout() {
        let frame = Frame::new(Channel::Files, 0x0102_0304, vec![0xAA]).unwrap();
        assert_eq!(frame.encode().unwrap(), [2, 1, 2, 3, 4, 0, 0, 0, 1, 0xAA]);
    }

    #[test]
    fn accepts_payload_at_the_limit() {
        let frame = Frame::new(Channel::Desktop, 0, vec![0; MAX_PAYLOAD_LEN]).unwrap();
        assert_eq!(Frame::decode(&frame.encode().unwrap()).unwrap(), frame);
    }

    #[test]
    fn rejects_oversized_payload_on_construction() {
        let result = Frame::new(Channel::Files, 1, vec![0; MAX_PAYLOAD_LEN + 1]);
        assert!(matches!(result, Err(ProtocolError::PayloadTooLarge { .. })));
    }

    #[test]
    fn rejects_declared_length_over_the_limit_without_reading_the_payload() {
        let mut bytes = vec![0; HEADER_LEN];
        bytes[5..9].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            Frame::decode(&bytes),
            Err(ProtocolError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn rejects_a_short_header() {
        assert_eq!(
            Frame::decode(&[0; HEADER_LEN - 1]),
            Err(ProtocolError::Truncated {
                len: HEADER_LEN - 1,
                needed: HEADER_LEN
            })
        );
    }

    #[test]
    fn rejects_a_truncated_payload() {
        let bytes = Frame::new(Channel::Terminal, 3, vec![9; 32])
            .unwrap()
            .encode()
            .unwrap();
        assert_eq!(
            Frame::decode(&bytes[..bytes.len() - 5]),
            Err(ProtocolError::LengthMismatch {
                declared: 32,
                actual: 27
            })
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut bytes = Frame::new(Channel::Audio, 3, vec![9; 4])
            .unwrap()
            .encode()
            .unwrap();
        bytes.push(0);
        assert!(matches!(
            Frame::decode(&bytes),
            Err(ProtocolError::LengthMismatch { .. })
        ));
    }

    #[test]
    fn rejects_an_unknown_channel() {
        let mut bytes = Frame::new(Channel::Audio, 3, vec![])
            .unwrap()
            .encode()
            .unwrap();
        bytes[0] = 200;
        assert_eq!(
            Frame::decode(&bytes),
            Err(ProtocolError::UnknownChannel(200))
        );
    }
}
