use wirehaus_capture::ScreenSource;
use wirehaus_input::InputSink;

use crate::clipboard::Factory;
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

#[cfg(target_os = "macos")]
pub fn connect(_display: Option<&str>) -> Result<Backend, ServerError> {
    Ok(Backend {
        screen: Box::new(wirehaus_capture::PollingScreen::new(
            wirehaus_capture::MacDisplay::connect()?,
        )),
        input: Box::new(wirehaus_input::MacInput::connect()?),
    })
}

#[cfg(windows)]
pub fn connect(_display: Option<&str>) -> Result<Backend, ServerError> {
    Ok(Backend {
        screen: Box::new(wirehaus_capture::WindowsScreen::connect(Box::new(|| {
            wirehaus_input::primary_display_size()
                .map_err(|error| wirehaus_capture::CaptureError::Connect(error.to_string()))
        }))?),
        input: Box::new(wirehaus_input::WindowsInput::connect()?),
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn connect(_display: Option<&str>) -> Result<Backend, ServerError> {
    Err(ServerError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
pub fn clipboard_factory(display: Option<String>) -> Factory {
    Box::new(move || {
        Ok(Box::new(wirehaus_clipboard::X11Clipboard::connect(
            display.as_deref(),
        )?))
    })
}

#[cfg(target_os = "macos")]
pub fn clipboard_factory(_display: Option<String>) -> Factory {
    Box::new(|| Ok(Box::new(wirehaus_clipboard::MacClipboard::connect())))
}

#[cfg(windows)]
pub fn clipboard_factory(_display: Option<String>) -> Factory {
    Box::new(|| Ok(Box::new(wirehaus_clipboard::WindowsClipboard::connect()?)))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn clipboard_factory(_display: Option<String>) -> Factory {
    Box::new(|| {
        Err(wirehaus_clipboard::ClipboardError::Connect(
            "this platform has no clipboard backend".to_owned(),
        ))
    })
}
