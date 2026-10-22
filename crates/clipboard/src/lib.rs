mod error;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "macos", all(test, unix)))]
mod process;
#[cfg(target_os = "linux")]
mod x11;

pub use error::ClipboardError;
#[cfg(target_os = "macos")]
pub use macos::MacClipboard;
#[cfg(target_os = "linux")]
pub use x11::X11Clipboard;

pub const MAX_TEXT_BYTES: usize = 32 * 1024;

pub trait Clipboard {
    fn read(&mut self) -> Result<Option<String>, ClipboardError>;
    fn write(&mut self, text: &str) -> Result<(), ClipboardError>;
}
