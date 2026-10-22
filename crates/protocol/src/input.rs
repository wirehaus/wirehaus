use crate::error::ProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    fn tag(self) -> u8 {
        match self {
            Button::Left => 0,
            Button::Middle => 1,
            Button::Right => 2,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(Button::Left),
            1 => Ok(Button::Middle),
            2 => Ok(Button::Right),
            other => Err(ProtocolError::InvalidInput(other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    PointerMove { x: u16, y: u16 },
    Button { button: Button, pressed: bool },
    Wheel { horizontal: i16, vertical: i16 },
    Key { code: u16, pressed: bool },
}

const POINTER_MOVE: u8 = 0;
const BUTTON: u8 = 1;
const WHEEL: u8 = 2;
const KEY: u8 = 3;

impl InputEvent {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match *self {
            InputEvent::PointerMove { x, y } => {
                let mut out = vec![POINTER_MOVE];
                out.extend_from_slice(&x.to_be_bytes());
                out.extend_from_slice(&y.to_be_bytes());
                out
            }
            InputEvent::Button { button, pressed } => {
                vec![BUTTON, button.tag(), u8::from(pressed)]
            }
            InputEvent::Wheel {
                horizontal,
                vertical,
            } => {
                let mut out = vec![WHEEL];
                out.extend_from_slice(&horizontal.to_be_bytes());
                out.extend_from_slice(&vertical.to_be_bytes());
                out
            }
            InputEvent::Key { code, pressed } => {
                let mut out = vec![KEY];
                out.extend_from_slice(&code.to_be_bytes());
                out.push(u8::from(pressed));
                out
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&tag, body) = bytes
            .split_first()
            .ok_or(ProtocolError::Truncated { len: 0, needed: 1 })?;
        match tag {
            POINTER_MOVE => {
                let [x0, x1, y0, y1] = fixed(tag, body)?;
                Ok(InputEvent::PointerMove {
                    x: u16::from_be_bytes([x0, x1]),
                    y: u16::from_be_bytes([y0, y1]),
                })
            }
            BUTTON => {
                let [button, pressed] = fixed(tag, body)?;
                Ok(InputEvent::Button {
                    button: Button::from_tag(button)?,
                    pressed: flag(pressed)?,
                })
            }
            WHEEL => {
                let [h0, h1, v0, v1] = fixed(tag, body)?;
                Ok(InputEvent::Wheel {
                    horizontal: i16::from_be_bytes([h0, h1]),
                    vertical: i16::from_be_bytes([v0, v1]),
                })
            }
            KEY => {
                let [c0, c1, pressed] = fixed(tag, body)?;
                Ok(InputEvent::Key {
                    code: u16::from_be_bytes([c0, c1]),
                    pressed: flag(pressed)?,
                })
            }
            other => Err(ProtocolError::UnknownInput(other)),
        }
    }
}

fn fixed<const N: usize>(tag: u8, body: &[u8]) -> Result<[u8; N], ProtocolError> {
    body.try_into().map_err(|_| ProtocolError::BadInputLength {
        tag,
        expected: N,
        actual: body.len(),
    })
}

fn flag(value: u8) -> Result<bool, ProtocolError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(ProtocolError::InvalidInput(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLES: [InputEvent; 6] = [
        InputEvent::PointerMove { x: 1919, y: 1079 },
        InputEvent::Button {
            button: Button::Right,
            pressed: true,
        },
        InputEvent::Button {
            button: Button::Left,
            pressed: false,
        },
        InputEvent::Wheel {
            horizontal: -3,
            vertical: 120,
        },
        InputEvent::Key {
            code: 30,
            pressed: true,
        },
        InputEvent::Key {
            code: 0,
            pressed: false,
        },
    ];

    #[test]
    fn round_trips_every_event() {
        for event in SAMPLES {
            assert_eq!(InputEvent::decode(&event.encode()).unwrap(), event);
        }
    }

    #[test]
    fn encodes_a_known_layout() {
        assert_eq!(
            InputEvent::Key {
                code: 0x0102,
                pressed: true
            }
            .encode(),
            [3, 1, 2, 1]
        );
        assert_eq!(
            InputEvent::PointerMove {
                x: 0x0102,
                y: 0x0304
            }
            .encode(),
            [0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn rejects_an_empty_payload() {
        assert_eq!(
            InputEvent::decode(&[]),
            Err(ProtocolError::Truncated { len: 0, needed: 1 })
        );
    }

    #[test]
    fn rejects_an_unknown_tag() {
        assert_eq!(
            InputEvent::decode(&[9, 0, 0]),
            Err(ProtocolError::UnknownInput(9))
        );
    }

    #[test]
    fn rejects_wrong_lengths() {
        for event in SAMPLES {
            let mut longer = event.encode();
            longer.push(0);
            assert!(matches!(
                InputEvent::decode(&longer),
                Err(ProtocolError::BadInputLength { .. })
            ));
            let mut shorter = event.encode();
            shorter.pop();
            assert!(matches!(
                InputEvent::decode(&shorter),
                Err(ProtocolError::BadInputLength { .. })
            ));
        }
    }

    #[test]
    fn rejects_a_flag_that_is_not_zero_or_one() {
        assert_eq!(
            InputEvent::decode(&[3, 0, 30, 2]),
            Err(ProtocolError::InvalidInput(2))
        );
    }

    #[test]
    fn rejects_an_unknown_button() {
        assert_eq!(
            InputEvent::decode(&[1, 7, 1]),
            Err(ProtocolError::InvalidInput(7))
        );
    }
}
