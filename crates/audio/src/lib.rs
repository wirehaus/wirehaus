mod encoder;
mod error;
mod source;

pub use encoder::Encoder;
pub use error::AudioError;
pub use source::{AudioSource, Interrupter, open_default};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
pub const FRAME_SAMPLES: usize = 960;
pub const FRAME_VALUES: usize = FRAME_SAMPLES * CHANNELS;
