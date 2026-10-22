use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tungstenite::{Error as WsError, Message, WebSocket};
use wirehaus_capture::{Rect, Region, Update, all_tiles, coalesce};
use wirehaus_encode::{JpegEncoder, TileEncoder};
use wirehaus_input::{InputError, InputSink};
use wirehaus_protocol::{Channel, DesktopMessage, Frame, ProtocolError, ViewerMessage};

use crate::error::ServerError;
use crate::pacer::Pacer;
use crate::platform::Backend;

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const MIN_READ_WAIT: Duration = Duration::from_millis(1);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGES_PER_TICK: usize = 256;

pub fn run(
    mut socket: WebSocket<TcpStream>,
    mut backend: Backend,
    mut encoder: JpegEncoder,
    mut pacer: Pacer,
    stop: &AtomicBool,
) -> Result<(), ServerError> {
    socket.get_ref().set_write_timeout(Some(WRITE_TIMEOUT))?;
    while !stop.load(Ordering::Relaxed) {
        let deadline = Instant::now() + FRAME_INTERVAL;
        pacer.tick();
        if pacer.may_send() {
            encoder.set_quality(pacer.quality());
            let update = backend.screen.next_update()?;
            let sent_at = Instant::now();
            if let Some(bytes) = send_update(&mut socket, &mut encoder, &update)? {
                let sequence = pacer.sent(sent_at, bytes);
                send_message(&mut socket, &DesktopMessage::FrameEnd { sequence })?;
            }
            socket.flush().or_else(ignore_would_block)?;
        }
        if pacer.stalled(Instant::now()) {
            return Err(ServerError::ViewerStalled);
        }
        if !receive(&mut socket, backend.input.as_mut(), &mut pacer, deadline)? {
            return Ok(());
        }
    }
    let _ = socket.close(None);
    Ok(())
}

fn send_update(
    socket: &mut WebSocket<TcpStream>,
    encoder: &mut JpegEncoder,
    update: &Update,
) -> Result<Option<usize>, ServerError> {
    let mut bytes = 0;
    if update.resized {
        bytes += send_message(
            socket,
            &DesktopMessage::Resize {
                width: update.width,
                height: update.height,
            },
        )?;
    }
    for region in &update.regions {
        bytes += send_region(socket, encoder, region)?;
    }
    Ok((update.resized || !update.regions.is_empty()).then_some(bytes))
}

fn send_region(
    socket: &mut WebSocket<TcpStream>,
    encoder: &mut JpegEncoder,
    region: &Region,
) -> Result<usize, ServerError> {
    coalesce(&all_tiles(&region.bitmap))
        .into_iter()
        .try_fold(0, |total, local| {
            Ok(total + send_rect(socket, encoder, region, local)?)
        })
}

fn send_rect(
    socket: &mut WebSocket<TcpStream>,
    encoder: &mut JpegEncoder,
    region: &Region,
    local: Rect,
) -> Result<usize, ServerError> {
    let message = DesktopMessage::Tile {
        x: region.rect.x.saturating_add(local.x),
        y: region.rect.y.saturating_add(local.y),
        width: local.width,
        height: local.height,
        codec: encoder.codec(),
        data: encoder.encode(&region.bitmap, local)?,
    };
    match Frame::new(Channel::Desktop, 0, message.encode()) {
        Ok(frame) => send_frame(socket, &frame),
        Err(ProtocolError::PayloadTooLarge { .. }) => match split(local) {
            Some(halves) => halves.into_iter().try_fold(0, |total, half| {
                Ok(total + send_rect(socket, encoder, region, half)?)
            }),
            None => Err(ProtocolError::PayloadTooLarge {
                len: usize::MAX,
                max: wirehaus_protocol::MAX_PAYLOAD_LEN,
            }
            .into()),
        },
        Err(error) => Err(error.into()),
    }
}

fn split(rect: Rect) -> Option<[Rect; 2]> {
    if rect.width > 1 {
        let left = rect.width / 2;
        Some([
            Rect {
                width: left,
                ..rect
            },
            Rect {
                x: rect.x + left,
                width: rect.width - left,
                ..rect
            },
        ])
    } else if rect.height > 1 {
        let top = rect.height / 2;
        Some([
            Rect {
                height: top,
                ..rect
            },
            Rect {
                y: rect.y + top,
                height: rect.height - top,
                ..rect
            },
        ])
    } else {
        None
    }
}

fn send_message(
    socket: &mut WebSocket<TcpStream>,
    message: &DesktopMessage,
) -> Result<usize, ServerError> {
    send_frame(socket, &Frame::new(Channel::Desktop, 0, message.encode())?)
}

fn send_frame(socket: &mut WebSocket<TcpStream>, frame: &Frame) -> Result<usize, ServerError> {
    let bytes = frame.encode()?;
    let len = bytes.len();
    socket.write(Message::Binary(bytes.into()))?;
    Ok(len)
}

fn receive(
    socket: &mut WebSocket<TcpStream>,
    input: &mut dyn InputSink,
    pacer: &mut Pacer,
    deadline: Instant,
) -> Result<bool, ServerError> {
    let mut handled = 0;
    while handled < MAX_MESSAGES_PER_TICK {
        let wait = deadline.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            break;
        }
        socket
            .get_ref()
            .set_read_timeout(Some(wait.max(MIN_READ_WAIT)))?;
        match socket.read() {
            Ok(Message::Binary(bytes)) => {
                handle_binary(&bytes, input, pacer)?;
                handled += 1;
            }
            Ok(Message::Close(_)) | Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => {
                return Ok(false);
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            Ok(_) => return Err(ServerError::UnexpectedMessage),
            Err(WsError::Io(e)) if is_timeout(e.kind()) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(true)
}

fn handle_binary(
    bytes: &[u8],
    input: &mut dyn InputSink,
    pacer: &mut Pacer,
) -> Result<(), ServerError> {
    let frame = Frame::decode(bytes)?;
    if frame.channel != Channel::Desktop {
        return Err(ServerError::UnexpectedChannel(frame.channel));
    }
    match ViewerMessage::decode(&frame.payload)? {
        ViewerMessage::Input(event) => match input.send(event) {
            Ok(()) | Err(InputError::PointerOutOfRange { .. } | InputError::KeyOutOfRange(_)) => {
                Ok(())
            }
            Err(error) => Err(error.into()),
        },
        ViewerMessage::FrameAck { sequence } => {
            pacer.acknowledged(sequence);
            Ok(())
        }
    }
}

fn is_timeout(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

fn ignore_would_block(error: WsError) -> Result<(), WsError> {
    match error {
        WsError::Io(e) if is_timeout(e.kind()) => Ok(()),
        other => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirehaus_protocol::{Button, InputEvent};

    #[derive(Default)]
    struct Recorder {
        events: Vec<InputEvent>,
        fail_with: Option<fn() -> InputError>,
    }

    impl InputSink for Recorder {
        fn send(&mut self, event: InputEvent) -> Result<(), InputError> {
            if let Some(make) = self.fail_with {
                return Err(make());
            }
            self.events.push(event);
            Ok(())
        }
    }

    fn ack_frame(sequence: u32) -> Vec<u8> {
        Frame::new(
            Channel::Desktop,
            0,
            ViewerMessage::FrameAck { sequence }.encode(),
        )
        .unwrap()
        .encode()
        .unwrap()
    }

    fn input_frame(channel: Channel, event: InputEvent) -> Vec<u8> {
        Frame::new(channel, 0, event.encode())
            .unwrap()
            .encode()
            .unwrap()
    }

    #[test]
    fn forwards_a_valid_input_event() {
        let mut sink = Recorder::default();
        let event = InputEvent::Button {
            button: Button::Left,
            pressed: true,
        };
        handle_binary(
            &input_frame(Channel::Desktop, event),
            &mut sink,
            &mut Pacer::new(70),
        )
        .unwrap();
        assert_eq!(sink.events, [event]);
    }

    #[test]
    fn an_acknowledgement_frees_a_slot_in_the_pacer() {
        let mut sink = Recorder::default();
        let mut pacer = Pacer::new(70);
        let now = Instant::now();
        let first = pacer.sent(now, crate::pacer::MAX_IN_FLIGHT_BYTES);
        assert!(!pacer.may_send());
        handle_binary(&ack_frame(first), &mut sink, &mut pacer).unwrap();
        assert!(pacer.may_send());
        assert_eq!(sink.events, []);
    }

    #[test]
    fn a_malformed_acknowledgement_is_an_error() {
        let mut sink = Recorder::default();
        let short = Frame::new(Channel::Desktop, 0, vec![4, 1, 2])
            .unwrap()
            .encode()
            .unwrap();
        assert!(handle_binary(&short, &mut sink, &mut Pacer::new(70)).is_err());
    }

    #[test]
    fn rejects_input_on_another_channel() {
        let mut sink = Recorder::default();
        let bytes = input_frame(Channel::Terminal, InputEvent::PointerMove { x: 1, y: 1 });
        assert!(matches!(
            handle_binary(&bytes, &mut sink, &mut Pacer::new(70)),
            Err(ServerError::UnexpectedChannel(Channel::Terminal))
        ));
        assert_eq!(sink.events, []);
    }

    #[test]
    fn rejects_malformed_frames_and_events() {
        let mut sink = Recorder::default();
        assert!(handle_binary(&[1, 2, 3], &mut sink, &mut Pacer::new(70)).is_err());
        let bad_event = Frame::new(Channel::Desktop, 0, vec![99])
            .unwrap()
            .encode()
            .unwrap();
        assert!(handle_binary(&bad_event, &mut sink, &mut Pacer::new(70)).is_err());
        assert_eq!(sink.events, []);
    }

    #[test]
    fn ignores_out_of_range_events_but_not_other_failures() {
        let bytes = input_frame(Channel::Desktop, InputEvent::PointerMove { x: 9, y: 9 });
        let mut out_of_range = Recorder {
            fail_with: Some(|| InputError::PointerOutOfRange { x: 9, y: 9 }),
            ..Recorder::default()
        };
        assert!(handle_binary(&bytes, &mut out_of_range, &mut Pacer::new(70)).is_ok());
        let mut broken = Recorder {
            fail_with: Some(|| InputError::Request("gone".into())),
            ..Recorder::default()
        };
        assert!(handle_binary(&bytes, &mut broken, &mut Pacer::new(70)).is_err());
    }

    #[test]
    fn splits_wide_regions_before_tall_ones() {
        let rect = Rect {
            x: 10,
            y: 20,
            width: 5,
            height: 8,
        };
        let [left, right] = split(rect).unwrap();
        assert_eq!((left.x, left.width), (10, 2));
        assert_eq!((right.x, right.width), (12, 3));
        assert_eq!(left.height, 8);
        let column = Rect { width: 1, ..rect };
        let [top, bottom] = split(column).unwrap();
        assert_eq!((top.y, top.height), (20, 4));
        assert_eq!((bottom.y, bottom.height), (24, 4));
    }

    #[test]
    fn a_single_pixel_cannot_be_split() {
        assert!(
            split(Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1
            })
            .is_none()
        );
    }
}
