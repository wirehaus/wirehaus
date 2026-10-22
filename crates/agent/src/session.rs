use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use wirehaus_capture::{Rect, Region, Update, all_tiles, coalesce};
use wirehaus_encode::{JpegEncoder, TileEncoder};
use wirehaus_input::{InputError, InputSink};
use wirehaus_protocol::{Channel, DesktopMessage, Frame, ProtocolError, ViewerMessage};

use crate::error::ServerError;
use crate::link::{Incoming, Link};
use crate::pacer::Pacer;
use crate::platform::Backend;

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGES_PER_TICK: usize = 256;
const REVOCATION_CHECK_TICKS: u32 = 30;

pub fn run(
    mut link: Link,
    mut backend: Backend,
    mut encoder: JpegEncoder,
    mut pacer: Pacer,
    stop: &AtomicBool,
    revoked: &dyn Fn() -> bool,
) -> Result<(), ServerError> {
    link.set_write_timeout(WRITE_TIMEOUT)?;
    let mut ticks: u32 = 0;
    while !stop.load(Ordering::Relaxed) {
        let deadline = Instant::now() + FRAME_INTERVAL;
        ticks = ticks.wrapping_add(1);
        if ticks.is_multiple_of(REVOCATION_CHECK_TICKS) && revoked() {
            link.close();
            return Err(ServerError::AccessRevoked);
        }
        pacer.tick();
        if pacer.may_send() {
            encoder.set_quality(pacer.quality());
            let update = backend.screen.next_update()?;
            let sent_at = Instant::now();
            if let Some(bytes) = send_update(&mut link, &mut encoder, &update)? {
                let sequence = pacer.sent(sent_at, bytes);
                send_message(&mut link, &DesktopMessage::FrameEnd { sequence })?;
            }
            link.flush()?;
        }
        if pacer.stalled(Instant::now()) {
            return Err(ServerError::ViewerStalled);
        }
        if !receive(&mut link, backend.input.as_mut(), &mut pacer, deadline)? {
            return Ok(());
        }
    }
    link.close();
    Ok(())
}

fn send_update(
    link: &mut Link,
    encoder: &mut JpegEncoder,
    update: &Update,
) -> Result<Option<usize>, ServerError> {
    let mut bytes = 0;
    if update.resized {
        bytes += send_message(
            link,
            &DesktopMessage::Resize {
                width: update.width,
                height: update.height,
            },
        )?;
    }
    for region in &update.regions {
        bytes += send_region(link, encoder, region)?;
    }
    Ok((update.resized || !update.regions.is_empty()).then_some(bytes))
}

fn send_region(
    link: &mut Link,
    encoder: &mut JpegEncoder,
    region: &Region,
) -> Result<usize, ServerError> {
    coalesce(&all_tiles(&region.bitmap))
        .into_iter()
        .try_fold(0, |total, local| {
            Ok(total + send_rect(link, encoder, region, local)?)
        })
}

fn send_rect(
    link: &mut Link,
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
        Ok(frame) => link.send_frame(&frame),
        Err(ProtocolError::PayloadTooLarge { .. }) => match split(local) {
            Some(halves) => halves.into_iter().try_fold(0, |total, half| {
                Ok(total + send_rect(link, encoder, region, half)?)
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

fn send_message(link: &mut Link, message: &DesktopMessage) -> Result<usize, ServerError> {
    link.send_frame(&Frame::new(Channel::Desktop, 0, message.encode())?)
}

fn receive(
    link: &mut Link,
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
        match link.receive(wait)? {
            Incoming::Frame(frame) => {
                handle_frame(&frame, input, pacer)?;
                handled += 1;
            }
            Incoming::Idle => {}
            Incoming::Closed => return Ok(false),
        }
    }
    Ok(true)
}

fn handle_frame(
    frame: &Frame,
    input: &mut dyn InputSink,
    pacer: &mut Pacer,
) -> Result<(), ServerError> {
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

    fn ack_frame(sequence: u32) -> Frame {
        Frame::new(
            Channel::Desktop,
            0,
            ViewerMessage::FrameAck { sequence }.encode(),
        )
        .unwrap()
    }

    fn input_frame(channel: Channel, event: InputEvent) -> Frame {
        Frame::new(channel, 0, event.encode()).unwrap()
    }

    #[test]
    fn forwards_a_valid_input_event() {
        let mut sink = Recorder::default();
        let event = InputEvent::Button {
            button: Button::Left,
            pressed: true,
        };
        handle_frame(
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
        handle_frame(&ack_frame(first), &mut sink, &mut pacer).unwrap();
        assert!(pacer.may_send());
        assert_eq!(sink.events, []);
    }

    #[test]
    fn a_malformed_acknowledgement_is_an_error() {
        let mut sink = Recorder::default();
        let short = Frame::new(Channel::Desktop, 0, vec![4, 1, 2]).unwrap();
        assert!(handle_frame(&short, &mut sink, &mut Pacer::new(70)).is_err());
    }

    #[test]
    fn rejects_input_on_another_channel() {
        let mut sink = Recorder::default();
        let frame = input_frame(Channel::Terminal, InputEvent::PointerMove { x: 1, y: 1 });
        assert!(matches!(
            handle_frame(&frame, &mut sink, &mut Pacer::new(70)),
            Err(ServerError::UnexpectedChannel(Channel::Terminal))
        ));
        assert_eq!(sink.events, []);
    }

    #[test]
    fn control_messages_are_not_accepted_after_authentication() {
        let mut sink = Recorder::default();
        let frame = Frame::new(
            Channel::Control,
            0,
            wirehaus_protocol::ControlMessage::AuthOk.encode(),
        )
        .unwrap();
        assert!(matches!(
            handle_frame(&frame, &mut sink, &mut Pacer::new(70)),
            Err(ServerError::UnexpectedChannel(Channel::Control))
        ));
    }

    #[test]
    fn rejects_malformed_events() {
        let mut sink = Recorder::default();
        let bad_event = Frame::new(Channel::Desktop, 0, vec![99]).unwrap();
        assert!(handle_frame(&bad_event, &mut sink, &mut Pacer::new(70)).is_err());
        assert_eq!(sink.events, []);
    }

    #[test]
    fn ignores_out_of_range_events_but_not_other_failures() {
        let frame = input_frame(Channel::Desktop, InputEvent::PointerMove { x: 9, y: 9 });
        let mut out_of_range = Recorder {
            fail_with: Some(|| InputError::PointerOutOfRange { x: 9, y: 9 }),
            ..Recorder::default()
        };
        assert!(handle_frame(&frame, &mut out_of_range, &mut Pacer::new(70)).is_ok());
        let mut broken = Recorder {
            fail_with: Some(|| InputError::Request("gone".into())),
            ..Recorder::default()
        };
        assert!(handle_frame(&frame, &mut broken, &mut Pacer::new(70)).is_err());
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
