mod error;
#[cfg(any(target_os = "macos", test))]
mod macmap;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(unix, windows, test))]
mod pressed;
#[cfg(windows)]
mod windows;
#[cfg(any(windows, test))]
mod winmap;
#[cfg(target_os = "linux")]
mod x11;

pub use error::InputError;
#[cfg(target_os = "macos")]
pub use macos::{MacInput, input_allowed, request_input};
#[cfg(windows)]
pub use windows::{WindowsInput, primary_display_size};
#[cfg(target_os = "linux")]
pub use x11::X11Input;

use wirehaus_protocol::InputEvent;

pub trait InputSink {
    fn send(&mut self, event: InputEvent) -> Result<(), InputError>;

    fn release_all(&mut self) -> Result<(), InputError> {
        Ok(())
    }

    fn select_monitor(&mut self, _index: u8) {}
}
