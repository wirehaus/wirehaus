use thiserror::Error;

#[derive(Debug, Error)]
pub enum InputError {
    #[error("cannot connect to the display: {0}")]
    Connect(String),
    #[error("the display has no XTEST extension")]
    MissingExtension,
    #[error("display request failed: {0}")]
    Request(String),
    #[error("key code {0} is outside the supported range")]
    KeyOutOfRange(u16),
    #[error("pointer position {x},{y} is outside the screen")]
    PointerOutOfRange { x: u16, y: u16 },
}
