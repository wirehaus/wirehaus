use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClipboardError {
    #[error("cannot connect to the display: {0}")]
    Connect(String),
    #[error("display request failed: {0}")]
    Request(String),
    #[error("the text is larger than {0} bytes")]
    TooLarge(usize),
    #[error("the clipboard owner did not answer in time")]
    Timeout,
    #[error("the clipboard could not be taken over")]
    NotOwner,
}
