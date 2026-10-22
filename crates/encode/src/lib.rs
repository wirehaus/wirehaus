mod error;
mod jpeg;

pub use error::EncodeError;
pub use jpeg::JpegEncoder;

use wirehaus_capture::{Bitmap, Rect};
use wirehaus_protocol::Codec;

pub trait TileEncoder {
    fn codec(&self) -> Codec;
    fn set_quality(&mut self, quality: u8);
    fn encode(&mut self, bitmap: &Bitmap, rect: Rect) -> Result<Vec<u8>, EncodeError>;
}
