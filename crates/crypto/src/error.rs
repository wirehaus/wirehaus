use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("the system random number generator failed")]
    Random,
    #[error("invalid base32 text")]
    InvalidBase32,
    #[error("the pairing code must be {expected} bytes but has {actual}")]
    InvalidPairingCode { expected: usize, actual: usize },
    #[error("the second factor secret must be {expected} bytes but has {actual}")]
    InvalidSecret { expected: usize, actual: usize },
}
