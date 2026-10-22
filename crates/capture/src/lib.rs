mod bitmap;
mod damage;
mod error;
#[cfg(target_os = "linux")]
mod x11;

pub use bitmap::{Bitmap, Rect};
pub use damage::{MAX_RUN_WIDTH, TILE_SIZE, all_tiles, changed_tiles, coalesce};
pub use error::CaptureError;
#[cfg(target_os = "linux")]
pub use x11::X11Screen;

pub trait ScreenSource {
    fn size(&self) -> (u16, u16);
    fn grab(&mut self) -> Result<Bitmap, CaptureError>;
}
