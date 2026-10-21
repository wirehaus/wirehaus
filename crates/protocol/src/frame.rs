use serde::{Deserialize, Serialize};

use crate::channel::Channel;
use crate::error::ProtocolError;

pub const MAX_PAYLOAD_LEN: usize = 1 << 20;
const MAX_HEADER_LEN: usize = 16;
const MAX_ENCODED_LEN: usize = MAX_PAYLOAD_LEN + MAX_HEADER_LEN;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        Ok(postcard::to_stdvec(self)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_ENCODED_LEN {
            return Err(ProtocolError::FrameTooLarge {
                len: bytes.len(),
                max: MAX_ENCODED_LEN,
            });
        }
        let frame: Self = postcard::from_bytes(bytes)?;
        check_payload(frame.payload.len())?;
        Ok(frame)
    }
}

fn check_payload(len: usize) -> Result<(), ProtocolError> {
    if len > MAX_PAYLOAD_LEN {
        return Err(ProtocolError::PayloadTooLarge {
            len,
            max: MAX_PAYLOAD_LEN,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_channel() {
        for channel in [
            Channel::Desktop,
            Channel::Terminal,
            Channel::Files,
            Channel::Clipboard,
            Channel::Audio,
            Channel::Capture,
        ] {
            let frame = Frame::new(channel, 7, vec![1, 2, 3]).unwrap();
            let decoded = Frame::decode(&frame.encode().unwrap()).unwrap();
            assert_eq!(decoded, frame);
        }
    }

    #[test]
    fn rejects_oversized_payload() {
        let result = Frame::new(Channel::Files, 1, vec![0; MAX_PAYLOAD_LEN + 1]);
        assert!(matches!(result, Err(ProtocolError::PayloadTooLarge { .. })));
    }

    #[test]
    fn rejects_oversized_input_before_parsing() {
        let result = Frame::decode(&vec![0; MAX_ENCODED_LEN + 1]);
        assert!(matches!(result, Err(ProtocolError::FrameTooLarge { .. })));
    }

    #[test]
    fn rejects_truncated_frame() {
        let frame = Frame::new(Channel::Terminal, 3, vec![9; 32]).unwrap();
        let bytes = frame.encode().unwrap();
        let result = Frame::decode(&bytes[..bytes.len() - 5]);
        assert!(matches!(result, Err(ProtocolError::Malformed(_))));
    }

    #[test]
    fn accepts_payload_at_the_limit() {
        let frame = Frame::new(Channel::Desktop, 0, vec![0; MAX_PAYLOAD_LEN]).unwrap();
        assert!(Frame::decode(&frame.encode().unwrap()).is_ok());
    }
}
