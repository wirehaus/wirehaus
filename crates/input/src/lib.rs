mod error;
#[cfg(any(target_os = "macos", test))]
mod macmap;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod pressed;
#[cfg(target_os = "linux")]
mod x11;

pub use error::InputError;
#[cfg(target_os = "macos")]
pub use macos::{MacInput, input_allowed, request_input};
#[cfg(target_os = "linux")]
pub use x11::X11Input;

use wirehaus_protocol::InputEvent;

pub trait InputSink {
    fn send(&mut self, event: InputEvent) -> Result<(), InputError>;

    fn release_all(&mut self) -> Result<(), InputError> {
        Ok(())
    }
}
