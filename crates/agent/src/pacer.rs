use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const MAX_IN_FLIGHT_BATCHES: usize = 8;
pub const MAX_IN_FLIGHT_BYTES: usize = 256 * 1024;
pub const MIN_QUALITY: u8 = 30;
pub const STALL_TIMEOUT: Duration = Duration::from_secs(45);

const WINDOW_TICKS: u32 = 15;
const CALM_WINDOWS_TO_RECOVER: u32 = 2;
const QUALITY_STEP_DOWN: u8 = 10;
const QUALITY_STEP_UP: u8 = 5;

#[derive(Debug)]
struct Flight {
    sequence: u32,
    sent_at: Instant,
    bytes: usize,
}

#[derive(Debug)]
pub struct Pacer {
    max_quality: u8,
    quality: u8,
    next_sequence: u32,
    in_flight: VecDeque<Flight>,
    in_flight_bytes: usize,
    window_ticks: u32,
    window_blocked: u32,
    calm_windows: u32,
}

impl Pacer {
    #[must_use]
    pub fn new(max_quality: u8) -> Self {
        let max_quality = max_quality.clamp(MIN_QUALITY, 100);
        Self {
            max_quality,
            quality: max_quality,
            next_sequence: 0,
            in_flight: VecDeque::new(),
            in_flight_bytes: 0,
            window_ticks: 0,
            window_blocked: 0,
            calm_windows: 0,
        }
    }

    #[must_use]
    pub fn may_send(&self) -> bool {
        self.in_flight.len() < MAX_IN_FLIGHT_BATCHES && self.in_flight_bytes < MAX_IN_FLIGHT_BYTES
    }

    #[must_use]
    pub fn quality(&self) -> u8 {
        self.quality
    }

    #[must_use]
    pub fn stalled(&self, now: Instant) -> bool {
        self.in_flight
            .front()
            .is_some_and(|oldest| now.saturating_duration_since(oldest.sent_at) > STALL_TIMEOUT)
    }

    pub fn sent(&mut self, now: Instant, bytes: usize) -> u32 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.in_flight_bytes += bytes;
        self.in_flight.push_back(Flight {
            sequence,
            sent_at: now,
            bytes,
        });
        sequence
    }

    pub fn acknowledged(&mut self, sequence: u32) {
        let Some(position) = self.in_flight.iter().position(|f| f.sequence == sequence) else {
            return;
        };
        let freed: usize = self.in_flight.drain(..=position).map(|f| f.bytes).sum();
        self.in_flight_bytes = self.in_flight_bytes.saturating_sub(freed);
    }

    pub fn tick(&mut self) {
        self.window_ticks += 1;
        if !self.may_send() {
            self.window_blocked += 1;
        }
        if self.window_ticks < WINDOW_TICKS {
            return;
        }
        if self.window_blocked * 2 >= self.window_ticks {
            self.slow_down();
        } else if self.window_blocked == 0 {
            self.recover();
        } else {
            self.calm_windows = 0;
        }
        self.window_ticks = 0;
        self.window_blocked = 0;
    }

    fn slow_down(&mut self) {
        self.calm_windows = 0;
        self.quality = self
            .quality
            .saturating_sub(QUALITY_STEP_DOWN)
            .max(MIN_QUALITY);
    }

    fn recover(&mut self) {
        self.calm_windows += 1;
        if self.calm_windows < CALM_WINDOWS_TO_RECOVER {
            return;
        }
        self.calm_windows = 0;
        self.quality = self
            .quality
            .saturating_add(QUALITY_STEP_UP)
            .min(self.max_quality);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill_batches(pacer: &mut Pacer, now: Instant) -> Vec<u32> {
        (0..MAX_IN_FLIGHT_BATCHES)
            .map(|_| pacer.sent(now, 100))
            .collect()
    }

    fn ticks(pacer: &mut Pacer, count: u32) {
        for _ in 0..count {
            pacer.tick();
        }
    }

    #[test]
    fn starts_at_full_quality_and_ready_to_send() {
        let pacer = Pacer::new(70);
        assert_eq!(pacer.quality(), 70);
        assert!(pacer.may_send());
    }

    #[test]
    fn limits_how_many_batches_are_in_flight() {
        let mut pacer = Pacer::new(70);
        let now = Instant::now();
        let sequences = fill_batches(&mut pacer, now);
        assert!(!pacer.may_send());
        pacer.acknowledged(sequences[0]);
        assert!(pacer.may_send());
    }

    #[test]
    fn limits_how_many_bytes_are_in_flight() {
        let mut pacer = Pacer::new(70);
        let now = Instant::now();
        let sequence = pacer.sent(now, MAX_IN_FLIGHT_BYTES);
        assert!(!pacer.may_send());
        pacer.acknowledged(sequence);
        assert!(pacer.may_send());
    }

    #[test]
    fn one_large_batch_is_allowed_to_overshoot_the_byte_limit() {
        let mut pacer = Pacer::new(70);
        assert!(pacer.may_send());
        pacer.sent(Instant::now(), MAX_IN_FLIGHT_BYTES * 4);
        assert!(!pacer.may_send());
    }

    #[test]
    fn an_acknowledgement_also_frees_every_earlier_batch() {
        let mut pacer = Pacer::new(70);
        let now = Instant::now();
        let sequences = fill_batches(&mut pacer, now);
        pacer.acknowledged(sequences[MAX_IN_FLIGHT_BATCHES - 1]);
        assert!(pacer.may_send());
        fill_batches(&mut pacer, now);
        assert!(!pacer.may_send());
    }

    #[test]
    fn ignores_unknown_and_repeated_acknowledgements() {
        let mut pacer = Pacer::new(70);
        let now = Instant::now();
        let first = pacer.sent(now, 100);
        pacer.acknowledged(first);
        pacer.acknowledged(first);
        pacer.acknowledged(9999);
        assert!(pacer.in_flight.is_empty());
        assert_eq!(pacer.in_flight_bytes, 0);
    }

    #[test]
    fn sequence_numbers_wrap_around() {
        let mut pacer = Pacer::new(70);
        pacer.next_sequence = u32::MAX;
        let now = Instant::now();
        assert_eq!(pacer.sent(now, 1), u32::MAX);
        assert_eq!(pacer.sent(now, 1), 0);
        pacer.acknowledged(0);
        assert!(pacer.in_flight.is_empty());
    }

    #[test]
    fn a_free_pipeline_keeps_full_quality() {
        let mut pacer = Pacer::new(70);
        ticks(&mut pacer, 1000);
        assert_eq!(pacer.quality(), 70);
    }

    #[test]
    fn a_pipeline_that_stays_full_lowers_quality_down_to_the_floor() {
        let mut pacer = Pacer::new(70);
        fill_batches(&mut pacer, Instant::now());
        ticks(&mut pacer, WINDOW_TICKS);
        assert_eq!(pacer.quality(), 60);
        ticks(&mut pacer, WINDOW_TICKS * 100);
        assert_eq!(pacer.quality(), MIN_QUALITY);
    }

    #[test]
    fn a_brief_block_does_not_change_quality() {
        let mut pacer = Pacer::new(70);
        let sequences = fill_batches(&mut pacer, Instant::now());
        ticks(&mut pacer, 3);
        pacer.acknowledged(sequences[MAX_IN_FLIGHT_BATCHES - 1]);
        ticks(&mut pacer, WINDOW_TICKS - 3);
        assert_eq!(pacer.quality(), 70);
    }

    #[test]
    fn it_recovers_once_the_pipeline_drains() {
        let mut pacer = Pacer::new(70);
        let sequences = fill_batches(&mut pacer, Instant::now());
        ticks(&mut pacer, WINDOW_TICKS * 3);
        assert!(pacer.quality() < 70);
        pacer.acknowledged(sequences[MAX_IN_FLIGHT_BATCHES - 1]);
        ticks(&mut pacer, WINDOW_TICKS * CALM_WINDOWS_TO_RECOVER * 20);
        assert_eq!(pacer.quality(), 70);
    }

    #[test]
    fn recovery_never_passes_the_configured_maximum() {
        let mut pacer = Pacer::new(50);
        ticks(&mut pacer, WINDOW_TICKS * 100);
        assert_eq!(pacer.quality(), 50);
    }

    #[test]
    fn the_maximum_quality_is_clamped_to_a_usable_range() {
        assert_eq!(Pacer::new(1).quality(), MIN_QUALITY);
        assert_eq!(Pacer::new(250).quality(), 100);
    }

    #[test]
    fn detects_a_viewer_that_stopped_answering() {
        let mut pacer = Pacer::new(70);
        let now = Instant::now();
        assert!(!pacer.stalled(now));
        pacer.sent(now, 10);
        assert!(!pacer.stalled(now + STALL_TIMEOUT));
        assert!(pacer.stalled(now + STALL_TIMEOUT + Duration::from_millis(1)));
    }

    #[test]
    fn nothing_in_flight_is_never_stalled() {
        let pacer = Pacer::new(70);
        assert!(!pacer.stalled(Instant::now() + Duration::from_secs(3600)));
    }
}
