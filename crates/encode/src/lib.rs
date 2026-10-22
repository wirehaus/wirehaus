mod error;
mod jpeg;
#[cfg(feature = "h264")]
mod video;

pub use error::EncodeError;
pub use jpeg::JpegEncoder;
#[cfg(feature = "h264")]
pub use video::{EncodedVideo, VideoEncoder};

use wirehaus_capture::{Bitmap, Rect};
use wirehaus_protocol::Codec;

pub trait TileEncoder {
    fn codec(&self) -> Codec;
    fn set_quality(&mut self, quality: u8);
    fn encode(&mut self, bitmap: &Bitmap, rect: Rect) -> Result<Vec<u8>, EncodeError>;
}
