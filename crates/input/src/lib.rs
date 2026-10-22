mod error;
#[cfg(target_os = "linux")]
mod x11;

pub use error::InputError;
#[cfg(target_os = "linux")]
pub use x11::X11Input;

use wirehaus_protocol::InputEvent;

pub trait InputSink {
    fn send(&mut self, event: InputEvent) -> Result<(), InputError>;
}
