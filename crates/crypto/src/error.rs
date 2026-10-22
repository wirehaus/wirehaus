use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("the system random number generator failed")]
    Random,
    #[error("invalid base32 text")]
    InvalidBase32,
    #[error("the pairing code must be {expected} bytes but has {actual}")]
    InvalidPairingCode { expected: usize, actual: usize },
    #[error("noise protocol error: {0}")]
    Noise(String),
    #[error("a key must be {expected} bytes but has {actual}")]
    InvalidKey { expected: usize, actual: usize },
    #[error("unknown connection mode {0}")]
    UnknownMode(u8),
    #[error("pairing is not open")]
    PairingNotOpen,
    #[error("handshake message of {len} bytes exceeds the limit of {max}")]
    HandshakeTooLarge { len: usize, max: usize },
    #[error("the device name is not valid")]
    InvalidDeviceName,
    #[error("message of {len} bytes exceeds the limit of {max}")]
    MessageTooLarge { len: usize, max: usize },
    #[error("invalid encrypted record: {0}")]
    InvalidRecord(&'static str),
    #[error("the session failed and cannot be used any more")]
    SessionClosed,
    #[error("the second factor secret must be {expected} bytes but has {actual}")]
    InvalidSecret { expected: usize, actual: usize },
}

impl From<snow::Error> for CryptoError {
    fn from(error: snow::Error) -> Self {
        CryptoError::Noise(error.to_string())
    }
}
