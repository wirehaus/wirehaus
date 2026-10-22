pub const FREE_FAILURES: u32 = 2;
pub const BASE_LOCK_SECONDS: u64 = 30;
pub const MAX_LOCK_SECONDS: u64 = 3600;
pub const SUSPEND_AT: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    Locked { retry_after: u64 },
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FailureTracker {
    consecutive: u32,
    locked_until: u64,
    suspended: bool,
}

impl FailureTracker {
    #[must_use]
    pub fn from_parts(consecutive: u32, locked_until: u64, suspended: bool) -> Self {
        Self {
            consecutive,
            locked_until,
            suspended,
        }
    }

    #[must_use]
    pub fn consecutive(&self) -> u32 {
        self.consecutive
    }

    #[must_use]
    pub fn locked_until(&self) -> u64 {
        self.locked_until
    }

    #[must_use]
    pub fn suspended(&self) -> bool {
        self.suspended
    }

    pub fn check(&self, now: u64) -> Result<(), Denied> {
        if self.suspended {
            return Err(Denied::Suspended);
        }
        if now < self.locked_until {
            return Err(Denied::Locked {
                retry_after: self.locked_until - now,
            });
        }
        Ok(())
    }

    pub fn record_failure(&mut self, now: u64) {
        self.consecutive = self.consecutive.saturating_add(1);
        if self.consecutive >= SUSPEND_AT {
            self.suspended = true;
        } else if self.consecutive > FREE_FAILURES {
            let doublings = (self.consecutive - FREE_FAILURES - 1).min(16);
            let seconds = (BASE_LOCK_SECONDS << doublings).min(MAX_LOCK_SECONDS);
            self.locked_until = now.saturating_add(seconds);
        }
    }

    pub fn record_success(&mut self) {
        self.consecutive = 0;
        self.locked_until = 0;
    }

    pub fn reactivate(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_tracker_allows_attempts() {
        assert_eq!(FailureTracker::default().check(0), Ok(()));
    }

    #[test]
    fn the_first_two_failures_cost_nothing() {
        let mut tracker = FailureTracker::default();
        tracker.record_failure(100);
        tracker.record_failure(100);
        assert_eq!(tracker.check(100), Ok(()));
    }

    #[test]
    fn the_lock_starts_at_the_third_failure_and_doubles() {
        let mut tracker = FailureTracker::default();
        let mut expected = BASE_LOCK_SECONDS;
        for _ in 0..FREE_FAILURES {
            tracker.record_failure(1000);
        }
        for _ in FREE_FAILURES + 1..SUSPEND_AT {
            tracker.record_failure(1000);
            assert_eq!(
                tracker.check(1000),
                Err(Denied::Locked {
                    retry_after: expected.min(MAX_LOCK_SECONDS)
                })
            );
            expected *= 2;
        }
    }

    #[test]
    fn the_lock_ends_on_time() {
        let mut tracker = FailureTracker::default();
        for _ in 0..3 {
            tracker.record_failure(1000);
        }
        assert_eq!(tracker.check(1029), Err(Denied::Locked { retry_after: 1 }));
        assert_eq!(tracker.check(1030), Ok(()));
    }

    #[test]
    fn the_lock_never_exceeds_an_hour() {
        let mut tracker = FailureTracker::from_parts(SUSPEND_AT - 2, 0, false);
        tracker.record_failure(0);
        assert!(matches!(
            tracker.check(0),
            Err(Denied::Locked { retry_after }) if retry_after <= MAX_LOCK_SECONDS
        ));
    }

    #[test]
    fn the_tenth_consecutive_failure_suspends_the_device() {
        let mut tracker = FailureTracker::default();
        for _ in 0..SUSPEND_AT {
            tracker.record_failure(0);
        }
        assert_eq!(tracker.check(u64::MAX), Err(Denied::Suspended));
        assert!(tracker.suspended());
    }

    #[test]
    fn a_success_resets_the_counter_but_not_a_suspension() {
        let mut tracker = FailureTracker::default();
        for _ in 0..5 {
            tracker.record_failure(0);
        }
        tracker.record_success();
        assert_eq!(tracker, FailureTracker::default());
        for _ in 0..SUSPEND_AT {
            tracker.record_failure(0);
        }
        tracker.record_success();
        assert_eq!(tracker.check(u64::MAX), Err(Denied::Suspended));
    }

    #[test]
    fn only_the_owner_reactivation_lifts_a_suspension() {
        let mut tracker = FailureTracker::default();
        for _ in 0..SUSPEND_AT {
            tracker.record_failure(0);
        }
        tracker.reactivate();
        assert_eq!(tracker.check(0), Ok(()));
        assert_eq!(tracker.consecutive(), 0);
    }

    #[test]
    fn the_counter_survives_persistence() {
        let mut tracker = FailureTracker::default();
        for _ in 0..4 {
            tracker.record_failure(500);
        }
        let restored = FailureTracker::from_parts(
            tracker.consecutive(),
            tracker.locked_until(),
            tracker.suspended(),
        );
        assert_eq!(restored, tracker);
    }
}
