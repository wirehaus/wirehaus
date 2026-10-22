use wirehaus_capture::ScreenSource;
use wirehaus_input::InputSink;

use crate::error::ServerError;

pub struct Backend {
    pub screen: Box<dyn ScreenSource>,
    pub input: Box<dyn InputSink>,
}

#[cfg(target_os = "linux")]
pub fn connect(display: Option<&str>) -> Result<Backend, ServerError> {
    Ok(Backend {
        screen: Box::new(wirehaus_capture::X11Screen::connect(display)?),
        input: Box::new(wirehaus_input::X11Input::connect(display)?),
    })
}

#[cfg(not(target_os = "linux"))]
pub fn connect(_display: Option<&str>) -> Result<Backend, ServerError> {
    Err(ServerError::UnsupportedPlatform)
}
