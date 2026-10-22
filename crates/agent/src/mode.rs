use std::collections::VecDeque;

pub const ENTER_FRACTION: f32 = 0.30;
pub const ENTER_WINDOW: usize = 6;
pub const ENTER_HITS: usize = 4;
pub const LEAVE_FRACTION: f32 = 0.05;
pub const LEAVE_TICKS: u32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Tiles,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    None,
    ToVideo,
    ToTiles,
}

#[derive(Debug)]
pub struct ModePolicy {
    mode: Mode,
    recent: VecDeque<bool>,
    calm_ticks: u32,
}

impl Default for ModePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl ModePolicy {
    #[must_use]
    pub fn new() -> Self {
        Self {
            mode: Mode::Tiles,
            recent: VecDeque::with_capacity(ENTER_WINDOW),
            calm_ticks: 0,
        }
    }

    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn observe(&mut self, fraction: f32) -> Transition {
        match self.mode {
            Mode::Tiles => self.observe_tiles(fraction),
            Mode::Video => self.observe_video(fraction),
        }
    }

    fn observe_tiles(&mut self, fraction: f32) -> Transition {
        if self.recent.len() == ENTER_WINDOW {
            self.recent.pop_front();
        }
        self.recent.push_back(fraction >= ENTER_FRACTION);
        if self.recent.iter().filter(|&&busy| busy).count() >= ENTER_HITS {
            self.mode = Mode::Video;
            self.recent.clear();
            self.calm_ticks = 0;
            return Transition::ToVideo;
        }
        Transition::None
    }

    fn observe_video(&mut self, fraction: f32) -> Transition {
        if fraction < LEAVE_FRACTION {
            self.calm_ticks += 1;
        } else {
            self.calm_ticks = 0;
        }
        if self.calm_ticks >= LEAVE_TICKS {
            self.mode = Mode::Tiles;
            self.calm_ticks = 0;
            return Transition::ToTiles;
        }
        Transition::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(policy: &mut ModePolicy, fraction: f32, times: usize) -> Vec<Transition> {
        (0..times).map(|_| policy.observe(fraction)).collect()
    }

    #[test]
    fn starts_with_tiles_and_stays_there_for_small_changes() {
        let mut policy = ModePolicy::new();
        let seen = feed(&mut policy, 0.02, 500);
        assert!(seen.iter().all(|t| *t == Transition::None));
        assert_eq!(policy.mode(), Mode::Tiles);
    }

    #[test]
    fn sustained_large_changes_switch_to_video() {
        let mut policy = ModePolicy::new();
        let seen = feed(&mut policy, 0.6, ENTER_HITS);
        assert_eq!(seen.last(), Some(&Transition::ToVideo));
        assert_eq!(
            seen.iter().filter(|t| **t == Transition::ToVideo).count(),
            1
        );
        assert_eq!(policy.mode(), Mode::Video);
    }

    #[test]
    fn a_single_large_change_does_not() {
        let mut policy = ModePolicy::new();
        assert_eq!(policy.observe(1.0), Transition::None);
        feed(&mut policy, 0.0, 50);
        assert_eq!(policy.mode(), Mode::Tiles);
    }

    #[test]
    fn a_few_isolated_spikes_inside_the_window_do_not() {
        let mut policy = ModePolicy::new();
        for _ in 0..40 {
            policy.observe(0.9);
            policy.observe(0.0);
            policy.observe(0.0);
        }
        assert_eq!(policy.mode(), Mode::Tiles);
    }

    #[test]
    fn video_ends_only_after_a_long_calm() {
        let mut policy = ModePolicy::new();
        feed(&mut policy, 0.8, ENTER_HITS);
        let seen = feed(&mut policy, 0.0, usize::try_from(LEAVE_TICKS).unwrap() - 1);
        assert!(seen.iter().all(|t| *t == Transition::None));
        assert_eq!(policy.mode(), Mode::Video);
        assert_eq!(policy.observe(0.0), Transition::ToTiles);
        assert_eq!(policy.mode(), Mode::Tiles);
    }

    #[test]
    fn activity_resets_the_calm_counter() {
        let mut policy = ModePolicy::new();
        feed(&mut policy, 0.8, ENTER_HITS);
        for _ in 0..10 {
            feed(&mut policy, 0.0, usize::try_from(LEAVE_TICKS).unwrap() - 1);
            policy.observe(0.2);
        }
        assert_eq!(policy.mode(), Mode::Video);
    }

    #[test]
    fn it_can_go_back_and_forth() {
        let mut policy = ModePolicy::new();
        for _ in 0..3 {
            feed(&mut policy, 0.9, ENTER_HITS);
            assert_eq!(policy.mode(), Mode::Video);
            feed(&mut policy, 0.0, usize::try_from(LEAVE_TICKS).unwrap());
            assert_eq!(policy.mode(), Mode::Tiles);
        }
    }

    #[test]
    fn a_reset_goes_back_to_tiles() {
        let mut policy = ModePolicy::new();
        feed(&mut policy, 0.9, ENTER_HITS);
        policy.reset();
        assert_eq!(policy.mode(), Mode::Tiles);
    }
}
