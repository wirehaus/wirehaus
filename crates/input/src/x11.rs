use std::time::{Duration, Instant};

use wirehaus_protocol::{Button, InputEvent};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as RandrExt;
use x11rb::protocol::xproto::{
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, KEY_PRESS_EVENT,
    KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT,
};
use x11rb::protocol::xtest::ConnectionExt;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::InputSink;
use crate::error::InputError;
use crate::pressed::Pressed;

const EVDEV_KEYCODE_OFFSET: u16 = 8;
const MAX_KEYCODE: u16 = 255;
const SCROLL_UP: u8 = 4;
const SCROLL_DOWN: u8 = 5;
const SCROLL_LEFT: u8 = 6;
const SCROLL_RIGHT: u8 = 7;
const MAX_WHEEL_NOTCHES: u16 = 32;
const SIZE_CHECK_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub struct X11Input {
    connection: RustConnection,
    root: u32,
    width: u16,
    height: u16,
    origin: (u16, u16),
    view: Option<(u16, u16)>,
    selected: Option<u8>,
    last_size_check: Option<Instant>,
    pressed: Pressed,
}

impl X11Input {
    pub fn connect(display: Option<&str>) -> Result<Self, InputError> {
        let (connection, number) =
            x11rb::connect(display).map_err(|e| InputError::Connect(e.to_string()))?;
        let screen = connection
            .setup()
            .roots
            .get(number)
            .ok_or_else(|| InputError::Connect(format!("no screen {number}")))?;
        let (root, width, height) = (screen.root, screen.width_in_pixels, screen.height_in_pixels);
        connection
            .xtest_get_version(2, 2)
            .map_err(|_| InputError::MissingExtension)?
            .reply()
            .map_err(|_| InputError::MissingExtension)?;
        Ok(Self {
            connection,
            root,
            width,
            height,
            origin: (0, 0),
            view: None,
            selected: None,
            last_size_check: None,
            pressed: Pressed::default(),
        })
    }

    fn refresh_size(&mut self) -> Result<(), InputError> {
        if self
            .last_size_check
            .is_some_and(|checked| checked.elapsed() < SIZE_CHECK_INTERVAL)
        {
            return Ok(());
        }
        self.last_size_check = Some(Instant::now());
        let geometry = self
            .connection
            .get_geometry(self.root)
            .map_err(request_error)?
            .reply()
            .map_err(request_error)?;
        let changed = geometry.width != self.width || geometry.height != self.height;
        self.width = geometry.width;
        self.height = geometry.height;
        if changed && let Some(index) = self.selected {
            self.select_monitor(index);
        }
        Ok(())
    }

    fn outside(&self, x: u16, y: u16) -> bool {
        let (width, height) = self.view.unwrap_or((self.width, self.height));
        x >= width || y >= height
    }

    fn monitors(&self) -> Vec<(u16, u16, u16, u16)> {
        let Ok(reply) = self
            .connection
            .randr_get_monitors(self.root, true)
            .map_err(request_error)
            .and_then(|cookie| cookie.reply().map_err(request_error))
        else {
            return Vec::new();
        };
        let mut items: Vec<_> = reply
            .monitors
            .iter()
            .filter(|monitor| {
                monitor.width > 0 && monitor.height > 0 && monitor.x >= 0 && monitor.y >= 0
            })
            .map(|monitor| {
                (
                    !monitor.primary,
                    monitor.x,
                    monitor.y,
                    monitor.width,
                    monitor.height,
                )
            })
            .collect();
        items.sort_by_key(|item| (item.0, item.1, item.2));
        if items.len() > 1 {
            items
                .retain(|item| (item.1, item.2, item.3, item.4) != (0, 0, self.width, self.height));
        }
        items
            .into_iter()
            .filter_map(|(_, x, y, width, height)| {
                Some((
                    u16::try_from(x).ok()?,
                    u16::try_from(y).ok()?,
                    width,
                    height,
                ))
            })
            .collect()
    }

    fn fake(&self, kind: u8, detail: u8, x: i16, y: i16) -> Result<(), InputError> {
        self.connection
            .xtest_fake_input(kind, detail, 0, self.root, x, y, 0)
            .map_err(request_error)?;
        Ok(())
    }

    fn click(&self, button: u8) -> Result<(), InputError> {
        self.fake(BUTTON_PRESS_EVENT, button, 0, 0)?;
        self.fake(BUTTON_RELEASE_EVENT, button, 0, 0)
    }

    fn scroll(&self, amount: i16, negative: u8, positive: u8) -> Result<(), InputError> {
        let button = if amount < 0 { negative } else { positive };
        for _ in 0..amount.unsigned_abs().min(MAX_WHEEL_NOTCHES) {
            self.click(button)?;
        }
        Ok(())
    }
}

impl InputSink for X11Input {
    fn select_monitor(&mut self, index: u8) {
        if let Some(&(x, y, width, height)) = self.monitors().get(usize::from(index)) {
            self.origin = (x, y);
            self.view = Some((width, height));
            self.selected = Some(index);
        } else {
            self.origin = (0, 0);
            self.view = None;
            self.selected = None;
        }
    }

    fn send(&mut self, event: InputEvent) -> Result<(), InputError> {
        match event {
            InputEvent::PointerMove { x, y } => {
                if self.outside(x, y) {
                    self.refresh_size()?;
                }
                if self.outside(x, y) {
                    return Err(InputError::PointerOutOfRange { x, y });
                }
                self.fake(
                    MOTION_NOTIFY_EVENT,
                    0,
                    to_coordinate(x.saturating_add(self.origin.0)),
                    to_coordinate(y.saturating_add(self.origin.1)),
                )?;
            }
            InputEvent::Button { button, pressed } => {
                let kind = if pressed {
                    BUTTON_PRESS_EVENT
                } else {
                    BUTTON_RELEASE_EVENT
                };
                self.fake(kind, button_number(button), 0, 0)?;
                self.pressed.button(button, pressed);
            }
            InputEvent::Wheel {
                horizontal,
                vertical,
            } => {
                self.scroll(vertical, SCROLL_DOWN, SCROLL_UP)?;
                self.scroll(horizontal, SCROLL_LEFT, SCROLL_RIGHT)?;
            }
            InputEvent::Key { code, pressed } => {
                let kind = if pressed {
                    KEY_PRESS_EVENT
                } else {
                    KEY_RELEASE_EVENT
                };
                self.fake(kind, keycode(code)?, 0, 0)?;
                self.pressed.key(code, pressed);
            }
        }
        self.connection.flush().map_err(request_error)
    }

    fn release_all(&mut self) -> Result<(), InputError> {
        let (keys, buttons) = self.pressed.take();
        for code in keys {
            self.fake(KEY_RELEASE_EVENT, keycode(code)?, 0, 0)?;
        }
        for button in buttons {
            self.fake(BUTTON_RELEASE_EVENT, button_number(button), 0, 0)?;
        }
        self.connection.sync().map_err(request_error)
    }
}

fn keycode(evdev: u16) -> Result<u8, InputError> {
    evdev
        .checked_add(EVDEV_KEYCODE_OFFSET)
        .filter(|code| (EVDEV_KEYCODE_OFFSET..=MAX_KEYCODE).contains(code))
        .and_then(|code| u8::try_from(code).ok())
        .ok_or(InputError::KeyOutOfRange(evdev))
}

fn button_number(button: Button) -> u8 {
    match button {
        Button::Left => 1,
        Button::Middle => 2,
        Button::Right => 3,
    }
}

fn to_coordinate(value: u16) -> i16 {
    i16::try_from(value).unwrap_or(i16::MAX)
}

fn request_error(error: impl std::fmt::Display) -> InputError {
    InputError::Request(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_evdev_codes_to_x11_keycodes() {
        assert_eq!(keycode(0).unwrap(), 8);
        assert_eq!(keycode(30).unwrap(), 38);
        assert_eq!(keycode(247).unwrap(), 255);
    }

    #[test]
    fn rejects_codes_outside_the_keycode_range() {
        assert!(matches!(keycode(248), Err(InputError::KeyOutOfRange(248))));
        assert!(matches!(
            keycode(u16::MAX),
            Err(InputError::KeyOutOfRange(_))
        ));
    }
}
