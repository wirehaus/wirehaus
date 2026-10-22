use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use wirehaus_clipboard::{Clipboard, ClipboardError};
use wirehaus_protocol::{ClipboardFailure, ClipboardMessage};

pub type Connection = Box<dyn Clipboard + Send>;
pub type Factory = Box<dyn Fn() -> Result<Connection, ClipboardError> + Send + Sync>;

pub struct Hub {
    factory: Factory,
    slot: Mutex<Option<Connection>>,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub").finish_non_exhaustive()
    }
}

impl Hub {
    #[must_use]
    pub fn new(factory: Factory) -> Self {
        Self {
            factory,
            slot: Mutex::new(None),
        }
    }

    pub fn read(&self) -> Result<Option<String>, ClipboardError> {
        self.run(|clipboard| clipboard.read())
    }

    pub fn write(&self, text: &str) -> Result<(), ClipboardError> {
        self.run(|clipboard| clipboard.write(text))
    }

    fn run<R>(
        &self,
        operation: impl FnOnce(&mut dyn Clipboard) -> Result<R, ClipboardError>,
    ) -> Result<R, ClipboardError> {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some((self.factory)()?);
        }
        let Some(connection) = slot.as_mut() else {
            return Err(ClipboardError::NotOwner);
        };
        let result = operation(connection.as_mut());
        if matches!(
            result,
            Err(ClipboardError::Connect(_) | ClipboardError::Request(_))
        ) {
            *slot = None;
        }
        result
    }
}

#[derive(Debug)]
pub struct Requests {
    hub: Arc<Hub>,
    sender: Sender<(u32, ClipboardMessage)>,
    results: Receiver<(u32, ClipboardMessage)>,
    pending: bool,
}

impl Requests {
    #[must_use]
    pub fn new(hub: Arc<Hub>) -> Self {
        let (sender, results) = channel();
        Self {
            hub,
            sender,
            results,
            pending: false,
        }
    }

    pub fn read(&mut self, stream: u32) -> bool {
        self.spawn(stream, |hub| match hub.read() {
            Ok(Some(text)) if !text.is_empty() => ClipboardMessage::Content(text),
            Ok(_) => ClipboardMessage::Empty,
            Err(error) => ClipboardMessage::Failed(failure(&error)),
        })
    }

    pub fn write(&mut self, stream: u32, text: String) -> bool {
        self.spawn(stream, move |hub| match hub.write(&text) {
            Ok(()) => ClipboardMessage::Written,
            Err(error) => ClipboardMessage::Failed(failure(&error)),
        })
    }

    pub fn drain(&mut self) -> Vec<(u32, ClipboardMessage)> {
        match self.results.try_recv() {
            Ok(reply) => {
                self.pending = false;
                vec![reply]
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => Vec::new(),
        }
    }

    fn spawn(
        &mut self,
        stream: u32,
        operation: impl FnOnce(&Hub) -> ClipboardMessage + Send + 'static,
    ) -> bool {
        if self.pending {
            return false;
        }
        self.pending = true;
        let (hub, sender) = (self.hub.clone(), self.sender.clone());
        thread::spawn(move || {
            let _ = sender.send((stream, operation(&hub)));
        });
        true
    }
}

fn failure(error: &ClipboardError) -> ClipboardFailure {
    match error {
        ClipboardError::TooLarge(_) => ClipboardFailure::TooLarge,
        ClipboardError::Timeout => ClipboardFailure::Timeout,
        _ => ClipboardFailure::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::*;

    #[derive(Default)]
    struct Memory(Option<String>);

    impl Clipboard for Memory {
        fn read(&mut self) -> Result<Option<String>, ClipboardError> {
            Ok(self.0.clone())
        }

        fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
            self.0 = Some(text.to_owned());
            Ok(())
        }
    }

    struct Shared(Arc<Mutex<Memory>>);

    impl Clipboard for Shared {
        fn read(&mut self) -> Result<Option<String>, ClipboardError> {
            self.0.lock().unwrap().read()
        }

        fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
            self.0.lock().unwrap().write(text)
        }
    }

    fn memory_hub() -> Arc<Hub> {
        let shared = Arc::new(Mutex::new(Memory::default()));
        Arc::new(Hub::new(Box::new(move || {
            Ok(Box::new(Shared(shared.clone())))
        })))
    }

    fn answer(requests: &mut Requests) -> (u32, ClipboardMessage) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(reply) = requests.drain().pop() {
                return reply;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("no reply");
    }

    #[test]
    fn writes_then_reads_and_reports_an_empty_clipboard() {
        let mut requests = Requests::new(memory_hub());
        assert!(requests.read(1));
        assert_eq!(answer(&mut requests), (1, ClipboardMessage::Empty));
        assert!(requests.write(2, "texto".to_owned()));
        assert_eq!(answer(&mut requests), (2, ClipboardMessage::Written));
        assert!(requests.read(3));
        assert_eq!(
            answer(&mut requests),
            (3, ClipboardMessage::Content("texto".to_owned()))
        );
    }

    #[test]
    fn only_one_request_runs_at_a_time() {
        let mut requests = Requests::new(memory_hub());
        assert!(requests.read(1));
        assert!(!requests.read(2));
        assert!(!requests.write(3, "x".to_owned()));
        answer(&mut requests);
        assert!(requests.read(4));
        answer(&mut requests);
    }

    #[test]
    fn the_content_outlives_a_session() {
        let hub = memory_hub();
        let mut first = Requests::new(hub.clone());
        first.write(1, "queda".to_owned());
        answer(&mut first);
        drop(first);
        let mut second = Requests::new(hub);
        second.read(1);
        assert_eq!(
            answer(&mut second),
            (1, ClipboardMessage::Content("queda".to_owned()))
        );
    }

    #[test]
    fn a_failing_connection_is_reported_and_retried_next_time() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = attempts.clone();
        let hub = Arc::new(Hub::new(Box::new(move || {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(ClipboardError::Connect("down".to_owned()))
            } else {
                Ok(Box::new(Memory::default()))
            }
        })));
        let mut requests = Requests::new(hub);
        requests.read(1);
        assert_eq!(
            answer(&mut requests),
            (1, ClipboardMessage::Failed(ClipboardFailure::Unavailable))
        );
        requests.read(2);
        assert_eq!(answer(&mut requests), (2, ClipboardMessage::Empty));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn errors_map_to_the_failures_the_viewer_understands() {
        assert_eq!(
            failure(&ClipboardError::TooLarge(1)),
            ClipboardFailure::TooLarge
        );
        assert_eq!(failure(&ClipboardError::Timeout), ClipboardFailure::Timeout);
        assert_eq!(
            failure(&ClipboardError::Request("x".to_owned())),
            ClipboardFailure::Unavailable
        );
    }
}
