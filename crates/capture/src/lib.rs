mod bitmap;
mod damage;
mod error;
mod framebuffer;
#[cfg(target_os = "macos")]
mod macos;
mod polling;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
mod x11;

pub use bitmap::{Bitmap, Rect};
pub use damage::{
    MAX_RUN_WIDTH, TILE_SIZE, all_tiles, changed_tiles, coalesce, tile_count, tiles_touching,
};
pub use error::CaptureError;
pub use framebuffer::FrameBuffer;
#[cfg(target_os = "macos")]
pub use macos::{MacDisplay, request_screen_capture, screen_capture_allowed};
pub use polling::{Backoff, Grabber, PollingScreen};
#[cfg(windows)]
pub use windows::{PrimarySize, WindowsDisplay};
#[cfg(target_os = "linux")]
pub use x11::X11Screen;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub rect: Rect,
    pub bitmap: Bitmap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub width: u16,
    pub height: u16,
    pub resized: bool,
    pub regions: Vec<Region>,
}

pub trait ScreenSource {
    fn next_update(&mut self) -> Result<Update, CaptureError>;

    fn wake(&mut self) {}
}
