use enigo::{Axis, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
use wirehaus_protocol::{Button, InputEvent};

use crate::InputSink;
use crate::error::InputError;
use crate::pressed::Pressed;
use crate::winmap::{scancode, wheel_notches};

pub fn primary_display_size() -> Result<(u16, u16), InputError> {
    let _ = enigo::set_dpi_awareness();
    let enigo = Enigo::new(&Settings::default()).map_err(request_error)?;
    let (width, height) = enigo.main_display().map_err(request_error)?;
    match (u16::try_from(width), u16::try_from(height)) {
        (Ok(width), Ok(height)) => Ok((width, height)),
        _ => Err(InputError::Request(format!(
            "the primary display of {width}x{height} is too large"
        ))),
    }
}

pub struct WindowsInput {
    enigo: Enigo,
    pressed: Pressed,
}

impl std::fmt::Debug for WindowsInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsInput")
            .finish_non_exhaustive()
    }
}

impl WindowsInput {
    pub fn connect() -> Result<Self, InputError> {
        let _ = enigo::set_dpi_awareness();
        let enigo = Enigo::new(&Settings::default()).map_err(request_error)?;
        Ok(Self {
            enigo,
            pressed: Pressed::default(),
        })
    }

    fn move_pointer(&mut self, x: u16, y: u16) -> Result<(), InputError> {
        let (wide, high) = self.enigo.main_display().map_err(request_error)?;
        if i32::from(x) >= wide || i32::from(y) >= high {
            return Err(InputError::PointerOutOfRange { x, y });
        }
        self.enigo
            .move_mouse(i32::from(x), i32::from(y), Coordinate::Abs)
            .map_err(request_error)
    }

    fn button(&mut self, button: Button, pressed: bool) -> Result<(), InputError> {
        self.enigo
            .button(to_button(button), direction(pressed))
            .map_err(request_error)
    }

    fn scroll(&mut self, horizontal: i16, vertical: i16) -> Result<(), InputError> {
        let (horizontal, vertical) = wheel_notches(horizontal, vertical);
        if vertical != 0 {
            self.enigo
                .scroll(vertical, Axis::Vertical)
                .map_err(request_error)?;
        }
        if horizontal != 0 {
            self.enigo
                .scroll(horizontal, Axis::Horizontal)
                .map_err(request_error)?;
        }
        Ok(())
    }

    fn key(&mut self, code: u16, pressed: bool) -> Result<(), InputError> {
        let scan = scancode(code).ok_or(InputError::KeyOutOfRange(code))?;
        self.enigo
            .raw(scan, direction(pressed))
            .map_err(request_error)
    }
}

impl InputSink for WindowsInput {
    fn send(&mut self, event: InputEvent) -> Result<(), InputError> {
        match event {
            InputEvent::PointerMove { x, y } => self.move_pointer(x, y),
            InputEvent::Button { button, pressed } => {
                self.button(button, pressed)?;
                self.pressed.button(button, pressed);
                Ok(())
            }
            InputEvent::Wheel {
                horizontal,
                vertical,
            } => self.scroll(horizontal, vertical),
            InputEvent::Key { code, pressed } => {
                self.key(code, pressed)?;
                self.pressed.key(code, pressed);
                Ok(())
            }
        }
    }

    fn release_all(&mut self) -> Result<(), InputError> {
        let (keys, buttons) = self.pressed.take();
        for code in keys {
            self.key(code, false)?;
        }
        for button in buttons {
            self.button(button, false)?;
        }
        Ok(())
    }
}

fn direction(pressed: bool) -> Direction {
    if pressed {
        Direction::Press
    } else {
        Direction::Release
    }
}

fn to_button(button: Button) -> enigo::Button {
    match button {
        Button::Left => enigo::Button::Left,
        Button::Right => enigo::Button::Right,
        Button::Middle => enigo::Button::Middle,
    }
}

fn request_error(error: impl std::fmt::Display) -> InputError {
    InputError::Request(error.to_string())
}
