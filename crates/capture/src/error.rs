use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("cannot connect to the display: {0}")]
    Connect(String),
    #[error("display request failed: {0}")]
    Request(String),
    #[error("unsupported pixel format: depth {depth}, {bits_per_pixel} bits per pixel")]
    UnsupportedFormat { depth: u8, bits_per_pixel: u8 },
    #[error("the display returned {actual} bytes but {expected} were expected")]
    ShortImage { expected: usize, actual: usize },
    #[error("bitmap of {width}x{height} is not valid")]
    InvalidSize { width: u16, height: u16 },
}
