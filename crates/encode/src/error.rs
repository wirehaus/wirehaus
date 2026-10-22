use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("region {x},{y} {width}x{height} is outside the {screen_width}x{screen_height} bitmap")]
    OutOfBounds {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        screen_width: u16,
        screen_height: u16,
    },
    #[error("encoder failed: {0}")]
    Encoder(String),
    #[error("the video encoder is not usable: {0}")]
    Video(String),
}
