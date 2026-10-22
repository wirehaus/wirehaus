use std::time::Instant;

use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
    ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use wirehaus_protocol::{Button, InputEvent};

use crate::InputSink;
use crate::error::InputError;
use crate::macmap::{
    ClickTracker, Modifier, Modifiers, mac_keycode, modifier_of, to_points, wheel_lines,
};
use crate::pressed::Pressed;

const TAP: CGEventTapLocation = CGEventTapLocation::HID;

#[must_use]
pub fn input_allowed() -> bool {
    macos_accessibility_client::accessibility::application_is_trusted()
}

#[must_use]
pub fn request_input() -> bool {
    macos_accessibility_client::accessibility::application_is_trusted_with_prompt()
}

pub struct MacInput {
    position: CGPoint,
    held: Vec<Button>,
    modifiers: Modifiers,
    clicks: ClickTracker,
    pressed: Pressed,
}

impl std::fmt::Debug for MacInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MacInput")
            .field("position", &self.position)
            .field("held", &self.held)
            .finish_non_exhaustive()
    }
}

impl MacInput {
    pub fn connect() -> Result<Self, InputError> {
        if !input_allowed() {
            return Err(InputError::PermissionDenied);
        }
        let position = CGEvent::new(source()?)
            .map_err(|()| request_error("cannot read the pointer position"))?
            .location();
        Ok(Self {
            position,
            held: Vec::new(),
            modifiers: Modifiers::default(),
            clicks: ClickTracker::default(),
            pressed: Pressed::default(),
        })
    }

    fn flags(&self) -> CGEventFlags {
        let mut flags = CGEventFlags::CGEventFlagNull;
        for (modifier, flag) in [
            (Modifier::Shift, CGEventFlags::CGEventFlagShift),
            (Modifier::Control, CGEventFlags::CGEventFlagControl),
            (Modifier::Option, CGEventFlags::CGEventFlagAlternate),
            (Modifier::Command, CGEventFlags::CGEventFlagCommand),
        ] {
            if self.modifiers.active(modifier) {
                flags |= flag;
            }
        }
        flags
    }

    fn move_pointer(&mut self, x: u16, y: u16) -> Result<(), InputError> {
        let display = CGDisplay::main();
        let (wide, high) = (display.pixels_wide(), display.pixels_high());
        if u64::from(x) >= wide || u64::from(y) >= high {
            return Err(InputError::PointerOutOfRange { x, y });
        }
        let bounds = display.bounds();
        let (width, height) = (bounds.size.width, bounds.size.height);
        self.position = CGPoint::new(
            to_points(x, width, u64_to_f64(wide)),
            to_points(y, height, u64_to_f64(high)),
        );
        let (kind, button) = match self.held.last() {
            Some(Button::Left) => (CGEventType::LeftMouseDragged, CGMouseButton::Left),
            Some(Button::Right) => (CGEventType::RightMouseDragged, CGMouseButton::Right),
            Some(Button::Middle) => (CGEventType::OtherMouseDragged, CGMouseButton::Center),
            None => (CGEventType::MouseMoved, CGMouseButton::Left),
        };
        let event = self.mouse_event(kind, button)?;
        event.post(TAP);
        Ok(())
    }

    fn press_button(&mut self, button: Button, pressed: bool) -> Result<(), InputError> {
        let (down, up, mac_button) = match button {
            Button::Left => (
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
                CGMouseButton::Left,
            ),
            Button::Right => (
                CGEventType::RightMouseDown,
                CGEventType::RightMouseUp,
                CGMouseButton::Right,
            ),
            Button::Middle => (
                CGEventType::OtherMouseDown,
                CGEventType::OtherMouseUp,
                CGMouseButton::Center,
            ),
        };
        let count = if pressed {
            self.held.retain(|&held| held != button);
            self.held.push(button);
            self.clicks
                .press(button, self.position.x, self.position.y, Instant::now())
        } else {
            self.held.retain(|&held| held != button);
            self.clicks.current()
        };
        let event = self.mouse_event(if pressed { down } else { up }, mac_button)?;
        event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, count);
        event.post(TAP);
        Ok(())
    }

    fn mouse_event(&self, kind: CGEventType, button: CGMouseButton) -> Result<CGEvent, InputError> {
        let event = CGEvent::new_mouse_event(source()?, kind, self.position, button)
            .map_err(|()| request_error("cannot create a mouse event"))?;
        event.set_flags(self.flags());
        Ok(event)
    }

    fn scroll(&self, horizontal: i16, vertical: i16) -> Result<(), InputError> {
        let (vertical, horizontal) = wheel_lines(horizontal, vertical);
        let event =
            CGEvent::new_scroll_event(source()?, ScrollEventUnit::LINE, 2, vertical, horizontal, 0)
                .map_err(|()| request_error("cannot create a scroll event"))?;
        event.set_flags(self.flags());
        event.post(TAP);
        Ok(())
    }

    fn key(&mut self, code: u16, pressed: bool) -> Result<(), InputError> {
        let keycode = mac_keycode(code).ok_or(InputError::KeyOutOfRange(code))?;
        self.modifiers.key(keycode, pressed);
        let event = CGEvent::new_keyboard_event(source()?, keycode, pressed)
            .map_err(|()| request_error("cannot create a keyboard event"))?;
        if modifier_of(keycode).is_some() {
            event.set_type(CGEventType::FlagsChanged);
        }
        event.set_flags(self.flags());
        event.post(TAP);
        Ok(())
    }
}

impl InputSink for MacInput {
    fn send(&mut self, event: InputEvent) -> Result<(), InputError> {
        match event {
            InputEvent::PointerMove { x, y } => self.move_pointer(x, y),
            InputEvent::Button { button, pressed } => {
                self.press_button(button, pressed)?;
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
            self.press_button(button, false)?;
        }
        Ok(())
    }
}

fn source() -> Result<CGEventSource, InputError> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|()| request_error("cannot create an event source"))
}

fn u64_to_f64(value: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f64
    }
}

fn request_error(message: &str) -> InputError {
    InputError::Request(message.to_owned())
}
