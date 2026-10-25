use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use wirehaus_input::{InputError, InputSink};
use wirehaus_protocol::{
    Channel, ClipboardFailure, ClipboardMessage, ControlMessage, DesktopMessage, EndReason,
    FilesMessage, Frame, TerminalMessage, ViewerMessage,
};

use crate::clipboard::{Hub, Requests};
use crate::error::ServerError;
use crate::files::Files;
use crate::link::{Incoming, Link};
use crate::pacer::Pacer;
use crate::platform::Backend;
use crate::state::ClipboardMode;
use crate::stream::{Stream, send_message};
use crate::terminal::{Event, Terminals, default_shell};

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGES_PER_TICK: usize = 256;
const REVOCATION_CHECK_TICKS: u32 = 30;

pub struct Policy<'a> {
    pub revoked: &'a dyn Fn() -> Option<EndReason>,
    pub idle_timeout: Duration,
    pub terminal_allowed: &'a dyn Fn() -> bool,
    pub file_roots: &'a dyn Fn() -> Vec<PathBuf>,
    pub clipboard_mode: &'a dyn Fn() -> ClipboardMode,
    pub audit: &'a dyn Fn(&str, &str),
}

struct Services<'a> {
    terminals: &'a mut Terminals,
    files: &'a mut Files,
    clipboard: &'a mut Requests,
    policy: &'a Policy<'a>,
}

pub fn run(
    link: Link,
    mut backend: Backend,
    stream: Stream,
    pacer: Pacer,
    stop: &AtomicBool,
    hub: &Arc<Hub>,
    policy: &Policy,
) -> Result<(), ServerError> {
    let result = serve(link, &mut backend, stream, pacer, stop, hub, policy);
    let _ = backend.input.release_all();
    result
}

fn serve(
    mut link: Link,
    backend: &mut Backend,
    mut stream: Stream,
    mut pacer: Pacer,
    stop: &AtomicBool,
    hub: &Arc<Hub>,
    policy: &Policy,
) -> Result<(), ServerError> {
    let mut terminals = Terminals::new(default_shell());
    let mut files = Files::new();
    let mut clipboard = Requests::new(hub.clone());
    link.set_write_timeout(WRITE_TIMEOUT)?;
    let mut ticks: u32 = 0;
    let mut viewer_decodes_video = false;
    let mut last_activity = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let deadline = Instant::now() + FRAME_INTERVAL;
        ticks = ticks.wrapping_add(1);
        if ticks.is_multiple_of(REVOCATION_CHECK_TICKS) {
            if let Some(reason) = (policy.revoked)() {
                end_session(&mut link, reason);
                return Err(ServerError::AccessRevoked);
            }
            withdraw_terminals(&mut link, &mut terminals, policy)?;
        }
        if files.active() == 0 && last_activity.elapsed() >= policy.idle_timeout {
            end_session(&mut link, EndReason::Idle);
            return Err(ServerError::IdleTimeout);
        }
        pacer.tick();
        if pacer.may_send() {
            stream.set_viewer_decodes_video(viewer_decodes_video);
            let update = backend.screen.next_update()?;
            let sent_at = Instant::now();
            if let Some(bytes) = stream.send(&mut link, &update, pacer.quality())? {
                let sequence = pacer.sent(sent_at, bytes);
                send_message(&mut link, &DesktopMessage::FrameEnd { sequence })?;
            }
            link.flush()?;
        }
        if pacer.stalled(Instant::now()) {
            return Err(ServerError::ViewerStalled);
        }
        pump_terminals(&mut link, &mut terminals)?;
        pump_files(&mut link, &mut files, policy)?;
        pump_clipboard(&mut link, &mut clipboard)?;
        let mut active = false;
        let open = receive(
            &mut link,
            backend.input.as_mut(),
            &mut pacer,
            (&mut viewer_decodes_video, &mut active),
            &mut Services {
                terminals: &mut terminals,
                files: &mut files,
                clipboard: &mut clipboard,
                policy,
            },
            deadline,
        )?;
        if active {
            last_activity = Instant::now();
            backend.screen.wake();
        }
        if !open {
            return Ok(());
        }
    }
    link.close();
    Ok(())
}

fn withdraw_terminals(
    link: &mut Link,
    terminals: &mut Terminals,
    policy: &Policy,
) -> Result<(), ServerError> {
    if terminals.count() == 0 || (policy.terminal_allowed)() {
        return Ok(());
    }
    (policy.audit)(
        "terminal_withdrawn",
        "the permission was removed during the session",
    );
    for stream in terminals.close_all() {
        let ended = TerminalMessage::Exited { code: -1 };
        link.send_frame(&Frame::new(Channel::Terminal, stream, ended.encode())?)?;
    }
    Ok(())
}

fn end_session(link: &mut Link, reason: EndReason) {
    let _ = link.send_control(ControlMessage::SessionEnded { reason });
    let _ = link.flush();
    link.close();
}

fn receive(
    link: &mut Link,
    input: &mut dyn InputSink,
    pacer: &mut Pacer,
    (video, active): (&mut bool, &mut bool),
    services: &mut Services,
    deadline: Instant,
) -> Result<bool, ServerError> {
    let mut handled = 0;
    while handled < MAX_MESSAGES_PER_TICK {
        let wait = deadline.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            break;
        }
        match link.receive(wait)? {
            Incoming::Frame(frame) if frame.channel == Channel::Terminal => {
                *active = true;
                let allowed = (services.policy.terminal_allowed)();
                terminal_frame(&frame, services.terminals, allowed, link)?;
                handled += 1;
            }
            Incoming::Frame(frame) if frame.channel == Channel::Clipboard => {
                *active = true;
                clipboard_frame(&frame, services, link)?;
                handled += 1;
            }
            Incoming::Frame(frame) if frame.channel == Channel::Files => {
                *active = true;
                files_frame(&frame, services, link)?;
                handled += 1;
            }
            Incoming::Frame(frame) => {
                *active |= handle_frame(&frame, input, pacer, video)?;
                handled += 1;
            }
            Incoming::Idle => {}
            Incoming::Closed => return Ok(false),
        }
    }
    Ok(true)
}

const TERMINAL_EVENTS_PER_TICK: usize = 32;
const FILE_CHUNKS_PER_TRANSFER_PER_TICK: usize = 4;

fn pump_files(link: &mut Link, files: &mut Files, policy: &Policy) -> Result<(), ServerError> {
    if files.active() == 0 {
        return Ok(());
    }
    let roots = (policy.file_roots)();
    send_file_replies(link, files.pump(&roots, FILE_CHUNKS_PER_TRANSFER_PER_TICK))
}

fn pump_clipboard(link: &mut Link, clipboard: &mut Requests) -> Result<(), ServerError> {
    for (stream, reply) in clipboard.drain() {
        link.send_frame(&Frame::new(Channel::Clipboard, stream, reply.encode())?)?;
    }
    Ok(())
}

fn clipboard_frame(
    frame: &Frame,
    services: &mut Services,
    link: &mut Link,
) -> Result<(), ServerError> {
    let stream = frame.stream;
    let message = ClipboardMessage::decode(&frame.payload)?;
    if let Some(reason) = start_clipboard(message, stream, services)? {
        let refusal = ClipboardMessage::Failed(reason);
        link.send_frame(&Frame::new(Channel::Clipboard, stream, refusal.encode())?)?;
    }
    Ok(())
}

fn start_clipboard(
    message: ClipboardMessage,
    stream: u32,
    services: &mut Services,
) -> Result<Option<ClipboardFailure>, ServerError> {
    let mode = (services.policy.clipboard_mode)();
    let audit = services.policy.audit;
    let started = match message {
        ClipboardMessage::Write(text) if mode.allows_write() => {
            let length = text.len();
            let started = services.clipboard.write(stream, text);
            if started {
                audit("clipboard_write", &format!("{length} bytes"));
            }
            started
        }
        ClipboardMessage::Read if mode.allows_read() => {
            let started = services.clipboard.read(stream);
            if started {
                audit("clipboard_read", "requested");
            }
            started
        }
        ClipboardMessage::Write(_) | ClipboardMessage::Read => {
            return Ok(Some(ClipboardFailure::NotAllowed));
        }
        ClipboardMessage::Content(_)
        | ClipboardMessage::Empty
        | ClipboardMessage::Written
        | ClipboardMessage::Failed(_) => return Err(ServerError::UnexpectedMessage),
    };
    Ok((!started).then_some(ClipboardFailure::Unavailable))
}

fn files_frame(frame: &Frame, services: &mut Services, link: &mut Link) -> Result<(), ServerError> {
    let message = FilesMessage::decode(&frame.payload)?;
    let roots = (services.policy.file_roots)();
    send_file_replies(link, services.files.handle(frame.stream, message, &roots))
}

fn send_file_replies(
    link: &mut Link,
    replies: Vec<(u32, FilesMessage)>,
) -> Result<(), ServerError> {
    for (stream, reply) in replies {
        link.send_frame(&Frame::new(Channel::Files, stream, reply.encode())?)?;
    }
    Ok(())
}

fn pump_terminals(link: &mut Link, terminals: &mut Terminals) -> Result<(), ServerError> {
    for event in terminals.drain(TERMINAL_EVENTS_PER_TICK) {
        let (stream, message) = match event {
            Event::Output { stream, data } => (stream, TerminalMessage::Output(data)),
            Event::Exited { stream, code } => (stream, TerminalMessage::Exited { code }),
        };
        link.send_frame(&Frame::new(Channel::Terminal, stream, message.encode())?)?;
    }
    Ok(())
}

fn terminal_frame(
    frame: &Frame,
    terminals: &mut Terminals,
    allowed: bool,
    link: &mut Link,
) -> Result<(), ServerError> {
    let stream = frame.stream;
    match TerminalMessage::decode(&frame.payload)? {
        TerminalMessage::Open { columns, rows } => {
            let outcome = if allowed {
                terminals.open(stream, columns, rows)
            } else {
                Err(wirehaus_protocol::RefusalReason::NotAllowed)
            };
            if let Err(reason) = outcome {
                let refusal = TerminalMessage::Refused(reason);
                link.send_frame(&Frame::new(Channel::Terminal, stream, refusal.encode())?)?;
            }
        }
        TerminalMessage::Input(data) => {
            terminals.input(stream, &data);
        }
        TerminalMessage::Resize { columns, rows } => {
            terminals.resize(stream, columns, rows);
        }
        TerminalMessage::Close => terminals.close(stream),
        TerminalMessage::Output(_)
        | TerminalMessage::Exited { .. }
        | TerminalMessage::Refused(_) => {
            return Err(ServerError::UnexpectedMessage);
        }
    }
    Ok(())
}

fn handle_frame(
    frame: &Frame,
    input: &mut dyn InputSink,
    pacer: &mut Pacer,
    video: &mut bool,
) -> Result<bool, ServerError> {
    if frame.channel != Channel::Desktop {
        return Err(ServerError::UnexpectedChannel(frame.channel));
    }
    match ViewerMessage::decode(&frame.payload)? {
        ViewerMessage::Input(event) => match input.send(event) {
            Ok(())
            | Err(
                InputError::PointerOutOfRange { .. }
                | InputError::KeyOutOfRange(_)
                | InputError::Blocked,
            ) => Ok(true),
            Err(error) => Err(error.into()),
        },
        ViewerMessage::FrameAck { sequence } => {
            pacer.acknowledged(sequence);
            Ok(false)
        }
        ViewerMessage::Capabilities { video: decodes } => {
            *video = decodes;
            Ok(false)
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
            &mut false,
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
        handle_frame(&ack_frame(first), &mut sink, &mut pacer, &mut false).unwrap();
        assert!(pacer.may_send());
        assert_eq!(sink.events, []);
    }

    #[test]
    fn a_malformed_acknowledgement_is_an_error() {
        let mut sink = Recorder::default();
        let short = Frame::new(Channel::Desktop, 0, vec![4, 1, 2]).unwrap();
        assert!(handle_frame(&short, &mut sink, &mut Pacer::new(70), &mut false).is_err());
    }

    #[test]
    fn the_declared_video_capability_follows_the_viewer_both_ways() {
        let mut sink = Recorder::default();
        let mut pacer = Pacer::new(70);
        let mut video = false;
        for declared in [true, false, true, false] {
            let frame = Frame::new(
                Channel::Desktop,
                0,
                ViewerMessage::Capabilities { video: declared }.encode(),
            )
            .unwrap();
            handle_frame(&frame, &mut sink, &mut pacer, &mut video).unwrap();
            assert_eq!(video, declared);
        }
        assert_eq!(sink.events, []);
    }

    #[test]
    fn rejects_input_on_another_channel() {
        let mut sink = Recorder::default();
        let frame = input_frame(Channel::Terminal, InputEvent::PointerMove { x: 1, y: 1 });
        assert!(matches!(
            handle_frame(&frame, &mut sink, &mut Pacer::new(70), &mut false),
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
            handle_frame(&frame, &mut sink, &mut Pacer::new(70), &mut false),
            Err(ServerError::UnexpectedChannel(Channel::Control))
        ));
    }

    #[test]
    fn rejects_malformed_events() {
        let mut sink = Recorder::default();
        let bad_event = Frame::new(Channel::Desktop, 0, vec![99]).unwrap();
        assert!(handle_frame(&bad_event, &mut sink, &mut Pacer::new(70), &mut false).is_err());
        assert_eq!(sink.events, []);
    }

    #[test]
    fn ignores_out_of_range_events_but_not_other_failures() {
        let frame = input_frame(Channel::Desktop, InputEvent::PointerMove { x: 9, y: 9 });
        let mut out_of_range = Recorder {
            fail_with: Some(|| InputError::PointerOutOfRange { x: 9, y: 9 }),
            ..Recorder::default()
        };
        assert!(handle_frame(&frame, &mut out_of_range, &mut Pacer::new(70), &mut false).is_ok());
        let mut blocked = Recorder {
            fail_with: Some(|| InputError::Blocked),
            ..Recorder::default()
        };
        assert!(handle_frame(&frame, &mut blocked, &mut Pacer::new(70), &mut false).is_ok());
        let mut broken = Recorder {
            fail_with: Some(|| InputError::Request("gone".into())),
            ..Recorder::default()
        };
        assert!(handle_frame(&frame, &mut broken, &mut Pacer::new(70), &mut false).is_err());
    }
}
