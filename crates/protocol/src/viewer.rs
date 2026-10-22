use crate::error::ProtocolError;
use crate::input::InputEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerMessage {
    Input(InputEvent),
    FrameAck { sequence: u32 },
}

const FRAME_ACK: u8 = 4;

impl ViewerMessage {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            ViewerMessage::Input(event) => event.encode(),
            ViewerMessage::FrameAck { sequence } => {
                let mut out = vec![FRAME_ACK];
                out.extend_from_slice(&sequence.to_be_bytes());
                out
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        match bytes.split_first() {
            Some((&FRAME_ACK, body)) => {
                let sequence: [u8; 4] =
                    body.try_into().map_err(|_| ProtocolError::BadInputLength {
                        tag: FRAME_ACK,
                        expected: 4,
                        actual: body.len(),
                    })?;
                Ok(ViewerMessage::FrameAck {
                    sequence: u32::from_be_bytes(sequence),
                })
            }
            _ => InputEvent::decode(bytes).map(ViewerMessage::Input),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Button;

    #[test]
    fn round_trips_input_and_acknowledgements() {
        for message in [
            ViewerMessage::Input(InputEvent::PointerMove { x: 5, y: 9 }),
            ViewerMessage::Input(InputEvent::Button {
                button: Button::Middle,
                pressed: true,
            }),
            ViewerMessage::FrameAck {
                sequence: 0xFFFF_FFFE,
            },
        ] {
            assert_eq!(ViewerMessage::decode(&message.encode()).unwrap(), message);
        }
    }

    #[test]
    fn keeps_the_input_layout_unchanged() {
        let event = InputEvent::Key {
            code: 30,
            pressed: true,
        };
        assert_eq!(ViewerMessage::Input(event).encode(), event.encode());
    }

    #[test]
    fn encodes_the_acknowledgement_layout() {
        assert_eq!(
            ViewerMessage::FrameAck {
                sequence: 0x0102_0304
            }
            .encode(),
            [4, 1, 2, 3, 4]
        );
    }

    #[test]
    fn rejects_an_acknowledgement_of_the_wrong_length() {
        assert!(matches!(
            ViewerMessage::decode(&[4, 1, 2, 3]),
            Err(ProtocolError::BadInputLength { tag: 4, .. })
        ));
        assert!(matches!(
            ViewerMessage::decode(&[4, 1, 2, 3, 4, 5]),
            Err(ProtocolError::BadInputLength { tag: 4, .. })
        ));
        assert!(matches!(
            ViewerMessage::decode(&[4]),
            Err(ProtocolError::BadInputLength { tag: 4, .. })
        ));
    }

    #[test]
    fn rejects_empty_and_unknown_messages() {
        assert!(ViewerMessage::decode(&[]).is_err());
        assert_eq!(
            ViewerMessage::decode(&[99]),
            Err(ProtocolError::UnknownInput(99))
        );
    }
}
