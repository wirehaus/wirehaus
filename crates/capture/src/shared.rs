use std::sync::{Arc, Mutex, PoisonError};

use crate::bitmap::Bitmap;
use crate::error::CaptureError;

pub enum Grab {
    Idle,
    Frame(Bitmap),
    Lost,
}

pub trait Duplicator: Send {
    fn grab(&mut self) -> Result<Grab, CaptureError>;
}

pub enum Polled {
    Idle,
    Frame(Arc<Bitmap>),
    Lost,
    Unavailable,
}

struct State<D> {
    source: Option<D>,
    latest: Option<Arc<Bitmap>>,
    generation: u64,
    leases: usize,
}

pub struct Shared<D> {
    state: Mutex<State<D>>,
}

impl<D: Duplicator> Shared<D> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                source: None,
                latest: None,
                generation: 0,
                leases: 0,
            }),
        }
    }

    pub fn lease(&self) -> Lease<'_, D> {
        self.lock().leases += 1;
        Lease {
            shared: self,
            seen: 0,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State<D>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<D: Duplicator> Default for Shared<D> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Lease<'a, D: Duplicator> {
    shared: &'a Shared<D>,
    seen: u64,
}

impl<D: Duplicator> Lease<'_, D> {
    pub fn ensure(
        &mut self,
        open: impl FnOnce() -> Result<D, CaptureError>,
    ) -> Result<(), CaptureError> {
        let mut state = self.shared.lock();
        if state.source.is_none() {
            state.source = Some(open()?);
        }
        Ok(())
    }

    pub fn poll(
        &mut self,
        open: impl FnOnce() -> Result<D, CaptureError>,
    ) -> Result<Polled, CaptureError> {
        let mut state = self.shared.lock();
        if state.source.is_none() {
            match open() {
                Ok(source) => state.source = Some(source),
                Err(_) => return Ok(Polled::Unavailable),
            }
        }
        let grabbed = match state.source.as_mut() {
            Some(source) => source.grab()?,
            None => return Ok(Polled::Unavailable),
        };
        match grabbed {
            Grab::Idle => {}
            Grab::Frame(bitmap) => {
                state.latest = Some(Arc::new(bitmap));
                state.generation += 1;
            }
            Grab::Lost => {
                state.source = None;
                state.latest = None;
                return Ok(Polled::Lost);
            }
        }
        match &state.latest {
            Some(latest) if state.generation != self.seen => {
                self.seen = state.generation;
                Ok(Polled::Frame(Arc::clone(latest)))
            }
            _ => Ok(Polled::Idle),
        }
    }
}

impl<D: Duplicator> Drop for Lease<'_, D> {
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.leases = state.leases.saturating_sub(1);
        if state.leases == 0 {
            state.source = None;
            state.latest = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct Script(VecDeque<Grab>);

    impl Duplicator for Script {
        fn grab(&mut self) -> Result<Grab, CaptureError> {
            Ok(self.0.pop_front().unwrap_or(Grab::Idle))
        }
    }

    fn bitmap(value: u8) -> Bitmap {
        Bitmap::new(1, 1, vec![value, value, value, 255]).unwrap()
    }

    fn script(items: Vec<Grab>) -> impl FnOnce() -> Result<Script, CaptureError> {
        move || Ok(Script(items.into()))
    }

    fn unavailable() -> Result<Script, CaptureError> {
        Err(CaptureError::Connect("no".to_owned()))
    }

    fn frame(polled: Polled) -> Arc<Bitmap> {
        match polled {
            Polled::Frame(bitmap) => bitmap,
            _ => panic!("expected a frame"),
        }
    }

    #[test]
    fn two_sessions_share_one_source_and_both_see_every_frame() {
        let shared = Shared::<Script>::new();
        let mut first = shared.lease();
        let mut second = shared.lease();
        let a = frame(first.poll(script(vec![Grab::Frame(bitmap(1))])).unwrap());
        let b = frame(second.poll(unavailable).unwrap());
        assert_eq!(*a, bitmap(1));
        assert_eq!(*b, bitmap(1));
        assert!(matches!(first.poll(unavailable).unwrap(), Polled::Idle));
        assert!(matches!(second.poll(unavailable).unwrap(), Polled::Idle));
    }

    #[test]
    fn a_session_that_polls_later_still_gets_the_newest_frame() {
        let shared = Shared::<Script>::new();
        let mut first = shared.lease();
        let mut second = shared.lease();
        first
            .poll(script(vec![Grab::Frame(bitmap(1)), Grab::Frame(bitmap(2))]))
            .unwrap();
        first.poll(unavailable).unwrap();
        assert_eq!(*frame(second.poll(unavailable).unwrap()), bitmap(2));
    }

    #[test]
    fn a_new_session_gets_the_current_frame_without_waiting_for_a_change() {
        let shared = Shared::<Script>::new();
        let mut first = shared.lease();
        first.poll(script(vec![Grab::Frame(bitmap(7))])).unwrap();
        let mut late = shared.lease();
        assert_eq!(*frame(late.poll(unavailable).unwrap()), bitmap(7));
    }

    #[test]
    fn losing_the_source_reports_it_once_and_the_next_poll_reopens_it() {
        let shared = Shared::<Script>::new();
        let mut lease = shared.lease();
        lease.poll(script(vec![Grab::Lost])).unwrap();
        assert!(matches!(
            lease.poll(unavailable).unwrap(),
            Polled::Unavailable
        ));
        assert_eq!(
            *frame(lease.poll(script(vec![Grab::Frame(bitmap(3))])).unwrap()),
            bitmap(3)
        );
    }

    #[test]
    fn the_source_is_released_when_the_last_session_leaves() {
        let shared = Shared::<Script>::new();
        {
            let mut one = shared.lease();
            let _two = shared.lease();
            one.poll(script(vec![Grab::Frame(bitmap(1))])).unwrap();
        }
        let mut again = shared.lease();
        assert!(matches!(
            again.poll(unavailable).unwrap(),
            Polled::Unavailable
        ));
    }

    #[test]
    fn the_source_stays_while_another_session_is_still_connected() {
        let shared = Shared::<Script>::new();
        let mut stays = shared.lease();
        stays.poll(script(vec![Grab::Frame(bitmap(1))])).unwrap();
        drop(shared.lease());
        assert!(matches!(stays.poll(unavailable).unwrap(), Polled::Idle));
    }

    #[test]
    fn ensure_reports_why_the_source_cannot_be_opened() {
        let shared = Shared::<Script>::new();
        let mut lease = shared.lease();
        assert!(lease.ensure(unavailable).is_err());
        assert!(lease.ensure(script(vec![])).is_ok());
    }
}
