use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, ExitStatus, MasterPty, PtySize, native_pty_system};
use wirehaus_protocol::{MAX_OUTPUT_CHUNK, RefusalReason};

pub const MAX_SESSIONS: usize = 4;
const QUEUE_CHUNKS: usize = 64;
const EXIT_GRACE: Duration = Duration::from_millis(500);
const EXIT_POLL: Duration = Duration::from_millis(10);

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Output { stream: u32, data: Vec<u8> },
    Exited { stream: u32, code: i32 },
}

type Tagged = (u64, Event);

struct Session {
    generation: u64,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

pub struct Terminals {
    sessions: HashMap<u32, Session>,
    sender: SyncSender<Tagged>,
    events: Receiver<Tagged>,
    program: String,
    next_generation: u64,
}

impl std::fmt::Debug for Terminals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminals")
            .field("open", &self.sessions.len())
            .finish_non_exhaustive()
    }
}

#[must_use]
pub fn default_shell() -> String {
    if cfg!(windows) {
        return std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned());
    }
    std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned())
}

impl Terminals {
    #[must_use]
    pub fn new(program: String) -> Self {
        let (sender, events) = sync_channel(QUEUE_CHUNKS);
        Self {
            sessions: HashMap::new(),
            sender,
            events,
            program,
            next_generation: 0,
        }
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    pub fn open(&mut self, stream: u32, columns: u16, rows: u16) -> Result<(), RefusalReason> {
        if self.sessions.contains_key(&stream) || self.sessions.len() >= MAX_SESSIONS {
            return Err(RefusalReason::TooManySessions);
        }
        let pair = native_pty_system()
            .openpty(size(columns, rows))
            .map_err(|_| RefusalReason::SpawnFailed)?;
        let mut command = CommandBuilder::new(&self.program);
        command.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|_| RefusalReason::SpawnFailed)?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|_| RefusalReason::SpawnFailed)?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|_| RefusalReason::SpawnFailed)?;
        let sender = self.sender.clone();
        let generation = self.next_generation;
        self.next_generation += 1;
        thread::spawn(move || {
            let mut buffer = vec![0_u8; MAX_OUTPUT_CHUNK];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let data = buffer[..count].to_vec();
                if sender
                    .send((generation, Event::Output { stream, data }))
                    .is_err()
                {
                    return;
                }
            }
            let _ = sender.send((generation, Event::Exited { stream, code: 0 }));
        });
        self.sessions.insert(
            stream,
            Session {
                generation,
                master: pair.master,
                writer,
                child,
            },
        );
        Ok(())
    }

    pub fn input(&mut self, stream: u32, data: &[u8]) -> bool {
        self.sessions.get_mut(&stream).is_some_and(|session| {
            session
                .writer
                .write_all(data)
                .and_then(|()| session.writer.flush())
                .is_ok()
        })
    }

    pub fn resize(&mut self, stream: u32, columns: u16, rows: u16) -> bool {
        self.sessions
            .get(&stream)
            .is_some_and(|session| session.master.resize(size(columns, rows)).is_ok())
    }

    pub fn close(&mut self, stream: u32) {
        if let Some(mut session) = self.sessions.remove(&stream) {
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
    }

    pub fn close_all(&mut self) -> Vec<u32> {
        let mut open: Vec<u32> = self.sessions.keys().copied().collect();
        open.sort_unstable();
        for &stream in &open {
            self.close(stream);
        }
        open
    }

    pub fn drain(&mut self, limit: usize) -> Vec<Event> {
        let mut out = Vec::new();
        while out.len() < limit {
            match self.events.try_recv() {
                Ok((generation, event)) => out.extend(self.accept(generation, event)),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    fn accept(&mut self, generation: u64, event: Event) -> Option<Event> {
        let (Event::Output { stream, .. } | Event::Exited { stream, .. }) = &event;
        let stream = *stream;
        if self
            .sessions
            .get(&stream)
            .is_none_or(|session| session.generation != generation)
        {
            return None;
        }
        match event {
            Event::Exited { .. } => Some(Event::Exited {
                stream,
                code: self.finish(stream),
            }),
            output @ Event::Output { .. } => Some(output),
        }
    }

    fn finish(&mut self, stream: u32) -> i32 {
        self.sessions
            .remove(&stream)
            .map_or(0, |mut session| reap(session.child.as_mut()))
    }
}

fn reap(child: &mut dyn Child) -> i32 {
    let deadline = Instant::now() + EXIT_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return exit_code(&status),
            Ok(None) if Instant::now() < deadline => thread::sleep(EXIT_POLL),
            _ => break,
        }
    }
    let _ = child.kill();
    child.wait().map_or(-1, |status| exit_code(&status))
}

fn exit_code(status: &ExitStatus) -> i32 {
    i32::try_from(status.exit_code()).unwrap_or(-1)
}

impl Drop for Terminals {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn size(columns: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols: columns,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn collect_until(terminals: &mut Terminals, wanted: impl Fn(&[u8]) -> bool) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            for event in terminals.drain(64) {
                if let Event::Output { data, .. } = event {
                    seen.extend_from_slice(&data);
                }
            }
            if wanted(&seen) {
                return seen;
            }
            thread::sleep(Duration::from_millis(20));
        }
        seen
    }

    fn contains(seen: &[u8], text: &str) -> bool {
        String::from_utf8_lossy(seen).contains(text)
    }

    fn shell() -> Terminals {
        Terminals::new("/bin/sh".to_owned())
    }

    #[test]
    fn a_shell_runs_commands_and_returns_their_output() {
        let mut terminals = shell();
        terminals.open(1, 80, 24).unwrap();
        assert!(terminals.input(1, b"echo wirehaus-$((20+22))\n"));
        let seen = collect_until(&mut terminals, |seen| contains(seen, "wirehaus-42"));
        assert!(
            contains(&seen, "wirehaus-42"),
            "{}",
            String::from_utf8_lossy(&seen)
        );
    }

    #[test]
    fn the_terminal_reports_its_size_and_follows_resizes() {
        let mut terminals = shell();
        terminals.open(1, 100, 30).unwrap();
        terminals.input(1, b"stty size\n");
        let seen = collect_until(&mut terminals, |seen| contains(seen, "30 100"));
        assert!(contains(&seen, "30 100"));
        assert!(terminals.resize(1, 120, 40));
        terminals.input(1, b"stty size\n");
        let seen = collect_until(&mut terminals, |seen| contains(seen, "40 120"));
        assert!(contains(&seen, "40 120"));
    }

    #[test]
    fn the_exit_code_of_the_shell_is_reported_and_the_session_is_gone() {
        let mut terminals = shell();
        terminals.open(7, 80, 24).unwrap();
        terminals.input(7, b"exit 3\n");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut code = None;
        while Instant::now() < deadline && code.is_none() {
            for event in terminals.drain(64) {
                if let Event::Exited {
                    stream,
                    code: value,
                } = event
                {
                    assert_eq!(stream, 7);
                    code = Some(value);
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(code, Some(3));
        assert_eq!(terminals.count(), 0);
        assert!(!terminals.input(7, b"x"));
    }

    #[test]
    fn at_most_four_sessions_and_stream_ids_are_unique() {
        let mut terminals = shell();
        for stream in 1..=u32::try_from(MAX_SESSIONS).unwrap() {
            terminals.open(stream, 80, 24).unwrap();
        }
        assert_eq!(
            terminals.open(99, 80, 24),
            Err(RefusalReason::TooManySessions)
        );
        terminals.close(1);
        assert!(terminals.open(99, 80, 24).is_ok());
        assert_eq!(
            terminals.open(99, 80, 24),
            Err(RefusalReason::TooManySessions)
        );
    }

    #[test]
    fn closing_a_session_kills_its_shell() {
        let mut terminals = shell();
        terminals.open(1, 80, 24).unwrap();
        terminals.input(1, b"sleep 1000\n");
        thread::sleep(Duration::from_millis(200));
        terminals.close(1);
        assert_eq!(terminals.count(), 0);
        assert!(!terminals.input(1, b"x"));
    }

    #[test]
    fn a_program_that_does_not_exist_is_refused() {
        let mut terminals = Terminals::new("/nonexistent/shell".to_owned());
        assert_eq!(terminals.open(1, 80, 24), Err(RefusalReason::SpawnFailed));
        assert_eq!(terminals.count(), 0);
    }

    #[test]
    fn operations_on_unknown_streams_do_nothing() {
        let mut terminals = shell();
        assert!(!terminals.input(5, b"x"));
        assert!(!terminals.resize(5, 80, 24));
        terminals.close(5);
        assert_eq!(terminals.drain(10), []);
    }

    #[test]
    fn a_slow_reader_does_not_lose_output() {
        let mut terminals = shell();
        terminals.open(1, 80, 24).unwrap();
        terminals.input(1, b"i=0; while [ $i -lt 3000 ]; do echo line-$i; i=$((i+1)); done; echo finished-$((1+1))marker\n");
        thread::sleep(Duration::from_millis(500));
        let seen = collect_until(&mut terminals, |seen| contains(seen, "finished-2marker"));
        assert!(contains(&seen, "line-0"));
        assert!(contains(&seen, "line-2999"));
        assert!(contains(&seen, "finished-2marker"));
    }

    #[test]
    fn a_stream_number_can_be_reused_after_closing_it() {
        let mut terminals = shell();
        terminals.open(1, 80, 24).unwrap();
        terminals.close(1);
        terminals.open(1, 80, 24).unwrap();
        thread::sleep(Duration::from_millis(300));
        assert!(
            terminals
                .drain(64)
                .iter()
                .all(|event| !matches!(event, Event::Exited { .. }))
        );
        assert_eq!(terminals.count(), 1);
        assert!(terminals.input(1, b"echo again-$((20+22))\n"));
        let seen = collect_until(&mut terminals, |seen| contains(seen, "again-42"));
        assert!(contains(&seen, "again-42"));
    }

    #[test]
    fn a_shell_that_drops_its_terminal_without_exiting_does_not_block_the_agent() {
        let mut terminals = shell();
        terminals.open(1, 80, 24).unwrap();
        terminals.input(1, b"exec sleep 30 <&- >&- 2>&-\n");
        let started = Instant::now();
        let mut exited = false;
        while !exited && started.elapsed() < Duration::from_secs(10) {
            exited = terminals
                .drain(64)
                .iter()
                .any(|event| matches!(event, Event::Exited { .. }));
            thread::sleep(Duration::from_millis(20));
        }
        assert!(exited);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(terminals.count(), 0);
    }

    #[test]
    fn closing_everything_reports_which_streams_were_open() {
        let mut terminals = shell();
        terminals.open(5, 80, 24).unwrap();
        terminals.open(2, 80, 24).unwrap();
        assert_eq!(terminals.close_all(), [2, 5]);
        assert_eq!(terminals.count(), 0);
        assert_eq!(terminals.close_all(), Vec::<u32>::new());
    }

    #[test]
    fn the_default_shell_is_never_empty() {
        assert_ne!(default_shell(), "");
    }
}
