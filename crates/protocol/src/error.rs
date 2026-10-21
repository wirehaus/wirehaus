use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("payload of {len} bytes exceeds the maximum of {max}")]
    PayloadTooLarge { len: usize, max: usize },
    #[error("frame of {len} bytes is shorter than the {needed}-byte header")]
    Truncated { len: usize, needed: usize },
    #[error("unknown channel tag {0}")]
    UnknownChannel(u8),
    #[error("header declares {declared} payload bytes but {actual} follow")]
    LengthMismatch { declared: usize, actual: usize },
}
