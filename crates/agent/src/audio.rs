use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use wirehaus_audio::{AudioError, AudioSource, Encoder, FRAME_VALUES, Interrupter};

const QUEUE_PACKETS: usize = 64;

pub type Opener = fn() -> Result<Box<dyn AudioSource>, AudioError>;

struct Running {
    packets: Receiver<Vec<u8>>,
    stop: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<String>>>,
    interrupt: Interrupter,
    thread: Option<JoinHandle<()>>,
}

pub struct Audio {
    open: Opener,
    running: Option<Running>,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Audio")
            .field("active", &self.running.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Drained {
    pub packets: Vec<Vec<u8>>,
    pub ended: Option<String>,
}

impl Audio {
    #[must_use]
    pub fn new(open: Opener) -> Self {
        Self {
            open,
            running: None,
        }
    }

    #[must_use]
    pub fn active(&self) -> bool {
        self.running.is_some()
    }

    pub fn start(&mut self) -> Result<(), AudioError> {
        if self.running.is_some() {
            return Ok(());
        }
        let source = (self.open)()?;
        let encoder = Encoder::new()?;
        let interrupt = source.interrupter();
        let (sender, packets) = sync_channel(QUEUE_PACKETS);
        let stop = Arc::new(AtomicBool::new(false));
        let reason = Arc::new(Mutex::new(None));
        let thread = {
            let stop = Arc::clone(&stop);
            let reason = Arc::clone(&reason);
            std::thread::spawn(move || {
                if let Err(error) = capture(source, encoder, &sender, &stop) {
                    *reason.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(error.to_string());
                }
            })
        };
        self.running = Some(Running {
            packets,
            stop,
            reason,
            interrupt,
            thread: Some(thread),
        });
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut running) = self.running.take() {
            running.stop.store(true, Ordering::Relaxed);
            (running.interrupt)();
            if let Some(thread) = running.thread.take() {
                let _ = thread.join();
            }
        }
    }

    pub fn drain(&mut self, max: usize) -> Drained {
        let Some(running) = &self.running else {
            return Drained::default();
        };
        let mut drained = Drained::default();
        while drained.packets.len() < max {
            match running.packets.try_recv() {
                Ok(packet) => drained.packets.push(packet),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let reason = running
                        .reason
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone();
                    drained.ended = Some(reason.unwrap_or_else(|| "the capture ended".to_owned()));
                    break;
                }
            }
        }
        if drained.ended.is_some() {
            self.stop();
        }
        drained
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop();
    }
}

fn capture(
    mut source: Box<dyn AudioSource>,
    mut encoder: Encoder,
    sender: &SyncSender<Vec<u8>>,
    stop: &AtomicBool,
) -> Result<(), AudioError> {
    let mut frame = vec![0f32; FRAME_VALUES];
    while !stop.load(Ordering::Relaxed) {
        source.read_frame(&mut frame)?;
        let packet = encoder.encode(&frame)?;
        match sender.try_send(packet) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => return Ok(()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    struct Tone {
        frames: Arc<AtomicUsize>,
        fail_after: Option<usize>,
        stopped: Arc<AtomicBool>,
    }

    impl AudioSource for Tone {
        fn read_frame(&mut self, frame: &mut [f32]) -> Result<(), AudioError> {
            if self.stopped.load(Ordering::Relaxed) {
                return Err(AudioError::Capture("interrupted".to_owned()));
            }
            let count = self.frames.fetch_add(1, Ordering::Relaxed);
            if self.fail_after.is_some_and(|limit| count >= limit) {
                return Err(AudioError::Capture("gone".to_owned()));
            }
            for (i, value) in frame.iter_mut().enumerate() {
                *value = if i % 8 < 4 { 0.3 } else { -0.3 };
            }
            std::thread::sleep(Duration::from_millis(1));
            Ok(())
        }

        fn interrupter(&self) -> Interrupter {
            let stopped = Arc::clone(&self.stopped);
            Box::new(move || stopped.store(true, Ordering::Relaxed))
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn steady() -> Result<Box<dyn AudioSource>, AudioError> {
        Ok(Box::new(Tone {
            frames: Arc::new(AtomicUsize::new(0)),
            fail_after: None,
            stopped: Arc::new(AtomicBool::new(false)),
        }))
    }

    #[allow(clippy::unnecessary_wraps)]
    fn dying() -> Result<Box<dyn AudioSource>, AudioError> {
        Ok(Box::new(Tone {
            frames: Arc::new(AtomicUsize::new(0)),
            fail_after: Some(3),
            stopped: Arc::new(AtomicBool::new(false)),
        }))
    }

    fn unavailable() -> Result<Box<dyn AudioSource>, AudioError> {
        Err(AudioError::Unavailable("no device".to_owned()))
    }

    fn wait_for(audio: &mut Audio, count: usize) -> Drained {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut all = Drained::default();
        while all.packets.len() < count && all.ended.is_none() && Instant::now() < deadline {
            let part = audio.drain(64);
            all.packets.extend(part.packets);
            all.ended = all.ended.or(part.ended);
            std::thread::sleep(Duration::from_millis(5));
        }
        all
    }

    #[test]
    fn it_delivers_packets_while_running_and_nothing_after_stopping() {
        let mut audio = Audio::new(steady);
        assert!(!audio.active());
        audio.start().unwrap();
        assert!(audio.active());
        let got = wait_for(&mut audio, 5);
        assert!(got.packets.len() >= 5 && got.ended.is_none());
        audio.stop();
        assert!(!audio.active());
        assert_eq!(audio.drain(8), Drained::default());
    }

    #[test]
    fn starting_twice_keeps_one_capture() {
        let mut audio = Audio::new(steady);
        audio.start().unwrap();
        audio.start().unwrap();
        assert!(audio.active());
    }

    #[test]
    fn a_missing_device_is_an_error_and_leaves_audio_off() {
        let mut audio = Audio::new(unavailable);
        assert!(audio.start().is_err());
        assert!(!audio.active());
    }

    #[test]
    fn a_dying_capture_is_reported_once_and_turns_audio_off() {
        let mut audio = Audio::new(dying);
        audio.start().unwrap();
        let got = wait_for(&mut audio, usize::MAX);
        assert!(
            got.ended
                .as_deref()
                .is_some_and(|reason| reason.contains("gone"))
        );
        assert!(!audio.active());
    }

    #[test]
    fn a_stalled_viewer_never_grows_the_queue_without_bound() {
        let mut audio = Audio::new(steady);
        audio.start().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let got = audio.drain(1000);
        assert!(got.packets.len() <= QUEUE_PACKETS, "{}", got.packets.len());
    }
}
