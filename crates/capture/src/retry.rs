use std::time::{Duration, Instant};

pub const RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
pub struct Retry {
    next: Option<Instant>,
}

impl Retry {
    #[must_use]
    pub fn waiting(&self, now: Instant) -> bool {
        self.next.is_some_and(|next| now < next)
    }

    #[must_use]
    pub fn pending(&self) -> bool {
        self.next.is_some()
    }

    pub fn failed(&mut self, now: Instant) {
        self.next = Some(now + RETRY_INTERVAL);
    }

    pub fn recovered(&mut self) {
        self.next = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_state_never_waits() {
        let retry = Retry::default();
        assert!(!retry.waiting(Instant::now()));
        assert!(!retry.pending());
    }

    #[test]
    fn after_a_failure_it_waits_one_interval_and_then_allows_a_new_attempt() {
        let start = Instant::now();
        let mut retry = Retry::default();
        retry.failed(start);
        assert!(retry.pending());
        assert!(retry.waiting(start + Duration::from_millis(900)));
        assert!(!retry.waiting(start + RETRY_INTERVAL));
        assert!(retry.pending());
    }

    #[test]
    fn recovering_clears_the_wait() {
        let start = Instant::now();
        let mut retry = Retry::default();
        retry.failed(start);
        retry.recovered();
        assert!(!retry.pending());
        assert!(!retry.waiting(start));
    }
}
