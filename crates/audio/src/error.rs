use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("system audio capture is not available on this system: {0}")]
    Unavailable(String),
    #[error("audio capture failed: {0}")]
    Capture(String),
    #[error("audio encoding failed: {0}")]
    Encode(String),
}
