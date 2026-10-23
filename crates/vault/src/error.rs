use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VaultError {
    #[error("the system vault is not available: {0}")]
    Unavailable(String),
    #[error("the system vault refused the operation: {0}")]
    Failed(String),
    #[error("the system vault returned something different from what was stored")]
    Mismatch,
    #[error("this system has no supported vault")]
    Unsupported,
}
