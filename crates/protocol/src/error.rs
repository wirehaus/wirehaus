use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("payload of {len} bytes exceeds the maximum of {max}")]
    PayloadTooLarge { len: usize, max: usize },
    #[error("frame of {len} bytes exceeds the maximum of {max}")]
    FrameTooLarge { len: usize, max: usize },
    #[error("malformed frame: {0}")]
    Malformed(#[from] postcard::Error),
}
