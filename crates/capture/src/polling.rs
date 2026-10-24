use std::time::{Duration, Instant};

use crate::bitmap::{Bitmap, Rect};
use crate::damage::{changed_tiles, coalesce};
use crate::error::CaptureError;
use crate::{Region, ScreenSource, Update};

pub trait Grabber {
    fn grab(&mut self) -> Result<Bitmap, CaptureError>;
}

const BUSY_PERCENT: usize = 2;
const BUSY_GRABS: u32 = 15;
const SLOW_GRABS: u32 = 60;
const MEDIUM_INTERVAL: Duration = Duration::from_millis(100);
const SLOW_INTERVAL: Duration = Duration::from_millis(250);

fn interval(quiet: u32) -> Duration {
    if quiet < BUSY_GRABS {
        Duration::ZERO
    } else if quiet < SLOW_GRABS {
        MEDIUM_INTERVAL
    } else {
        SLOW_INTERVAL
    }
}

#[derive(Debug, Default)]
pub struct Backoff {
    quiet: u32,
    last: Option<Instant>,
}

impl Backoff {
    #[must_use]
    pub fn due(&self, now: Instant) -> bool {
        self.last
            .is_none_or(|last| now.saturating_duration_since(last) >= interval(self.quiet))
    }

    pub fn observe(&mut self, now: Instant, busy: bool) {
        self.last = Some(now);
        self.quiet = if busy {
            0
        } else {
            self.quiet.saturating_add(1)
        };
    }

    pub fn wake(&mut self) {
        self.quiet = 0;
    }
}

#[derive(Debug)]
pub struct PollingScreen<G> {
    grabber: G,
    previous: Option<Bitmap>,
    backoff: Backoff,
}

impl<G: Grabber> PollingScreen<G> {
    pub fn new(grabber: G) -> Self {
        Self {
            grabber,
            previous: None,
            backoff: Backoff::default(),
        }
    }
}

impl<G: Grabber> ScreenSource for PollingScreen<G> {
    fn next_update(&mut self) -> Result<Update, CaptureError> {
        let now = Instant::now();
        if let Some(previous) = &self.previous
            && !self.backoff.due(now)
        {
            return Ok(Update {
                width: previous.width(),
                height: previous.height(),
                resized: false,
                regions: Vec::new(),
            });
        }
        let current = self.grabber.grab()?;
        let (width, height) = (current.width(), current.height());
        let same_size = self
            .previous
            .as_ref()
            .is_some_and(|previous| previous.width() == width && previous.height() == height);
        let regions = match (&self.previous, same_size) {
            (Some(previous), true) => changed_regions(previous, &current)?,
            _ => vec![Region {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
                bitmap: current.clone(),
            }],
        };
        self.backoff
            .observe(now, !same_size || is_busy(&regions, width, height));
        self.previous = Some(current);
        Ok(Update {
            width,
            height,
            resized: !same_size,
            regions,
        })
    }

    fn wake(&mut self) {
        self.backoff.wake();
    }
}

pub(crate) fn is_busy(regions: &[Region], width: u16, height: u16) -> bool {
    let changed: usize = regions
        .iter()
        .map(|region| usize::from(region.rect.width) * usize::from(region.rect.height))
        .sum();
    changed * 100 >= usize::from(width) * usize::from(height) * BUSY_PERCENT
}

pub(crate) fn changed_regions(
    previous: &Bitmap,
    current: &Bitmap,
) -> Result<Vec<Region>, CaptureError> {
    coalesce(&changed_tiles(previous, current))
        .into_iter()
        .map(|rect| {
            Bitmap::new(rect.width, rect.height, current.crop(rect))
                .map(|bitmap| Region { rect, bitmap })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct Script(VecDeque<Bitmap>);

    impl Grabber for Script {
        fn grab(&mut self) -> Result<Bitmap, CaptureError> {
            self.0.pop_front().ok_or(CaptureError::PermissionDenied)
        }
    }

    fn solid(width: u16, height: u16, value: u8) -> Bitmap {
        Bitmap::new(
            width,
            height,
            vec![value; usize::from(width) * usize::from(height) * 4],
        )
        .unwrap()
    }

    fn screen(frames: Vec<Bitmap>) -> PollingScreen<Script> {
        PollingScreen::new(Script(frames.into()))
    }

    #[test]
    fn the_first_update_is_a_full_frame_and_reports_the_size() {
        let mut screen = screen(vec![solid(100, 70, 1)]);
        let update = screen.next_update().unwrap();
        assert!(update.resized);
        assert_eq!((update.width, update.height), (100, 70));
        assert_eq!(update.regions.len(), 1);
        assert_eq!(update.regions[0].bitmap, solid(100, 70, 1));
    }

    #[test]
    fn an_unchanged_screen_sends_nothing() {
        let mut screen = screen(vec![solid(100, 70, 1), solid(100, 70, 1)]);
        screen.next_update().unwrap();
        let update = screen.next_update().unwrap();
        assert!(!update.resized);
        assert_eq!(update.regions.len(), 0);
    }

    #[test]
    fn only_the_changed_tiles_are_sent() {
        let first = solid(200, 200, 1);
        let mut second = first.clone();
        second.paste(70, 70, &solid(4, 4, 9)).unwrap();
        let mut screen = screen(vec![first, second.clone()]);
        screen.next_update().unwrap();
        let update = screen.next_update().unwrap();
        assert!(!update.resized);
        let total: usize = update
            .regions
            .iter()
            .map(|r| usize::from(r.rect.width) * usize::from(r.rect.height))
            .sum();
        assert!(total > 0 && total < 200 * 200);
        for region in &update.regions {
            assert_eq!(region.bitmap.bgra(), second.crop(region.rect));
        }
    }

    #[test]
    fn a_new_size_sends_a_full_frame_again() {
        let mut screen = screen(vec![solid(100, 70, 1), solid(120, 80, 1)]);
        screen.next_update().unwrap();
        let update = screen.next_update().unwrap();
        assert!(update.resized);
        assert_eq!((update.width, update.height), (120, 80));
        assert_eq!(update.regions.len(), 1);
    }

    #[test]
    fn polling_slows_down_when_nothing_changes_and_speeds_up_when_woken() {
        let frames: Vec<Bitmap> = (0..200).map(|_| solid(100, 70, 1)).collect();
        let mut screen = screen(frames);
        for _ in 0..=BUSY_GRABS {
            screen.next_update().unwrap();
        }
        let consumed = screen.grabber.0.len();
        let update = screen.next_update().unwrap();
        assert_eq!(update.regions.len(), 0);
        assert_eq!((update.width, update.height), (100, 70));
        assert_eq!(screen.grabber.0.len(), consumed);
        screen.wake();
        screen.next_update().unwrap();
        assert_eq!(screen.grabber.0.len(), consumed - 1);
    }

    #[test]
    fn a_change_resets_the_backoff() {
        let mut backoff = Backoff::default();
        let start = Instant::now();
        assert!(backoff.due(start));
        for tick in 0..BUSY_GRABS {
            backoff.observe(start, tick == 0);
        }
        assert!(backoff.due(start));
        backoff.observe(start, false);
        assert!(!backoff.due(start));
        assert!(!backoff.due(start + MEDIUM_INTERVAL / 2));
        assert!(backoff.due(start + MEDIUM_INTERVAL));
        backoff.observe(start, true);
        assert!(backoff.due(start));
    }

    #[test]
    fn the_interval_grows_in_two_steps_and_wake_returns_to_full_rate() {
        let mut backoff = Backoff::default();
        let start = Instant::now();
        for _ in 0..SLOW_GRABS {
            backoff.observe(start, false);
        }
        assert!(!backoff.due(start + MEDIUM_INTERVAL));
        assert!(backoff.due(start + SLOW_INTERVAL));
        backoff.wake();
        assert!(backoff.due(start));
    }

    fn reads_before_throttling() -> usize {
        usize::try_from(BUSY_GRABS).unwrap() + 1
    }

    #[test]
    fn small_changes_do_not_keep_the_polling_at_full_rate() {
        let first = solid(800, 600, 1);
        let mut frames = vec![first.clone()];
        for step in 1..reads_before_throttling() + 4 {
            let mut next = first.clone();
            next.paste(10, 10, &solid(4, 4, u8::try_from(step).unwrap()))
                .unwrap();
            frames.push(next);
        }
        let mut screen = screen(frames);
        for _ in 0..reads_before_throttling() {
            assert!(screen.next_update().unwrap().regions.len() <= 1);
        }
        let consumed = screen.grabber.0.len();
        assert_eq!(screen.next_update().unwrap().regions.len(), 0);
        assert_eq!(screen.grabber.0.len(), consumed);
    }

    #[test]
    fn after_a_large_change_the_polling_continues_at_full_rate() {
        let mut frames = vec![solid(800, 600, 1); reads_before_throttling()];
        frames.push(solid(800, 600, 9));
        frames.push(solid(800, 600, 9));
        let mut screen = screen(frames);
        for _ in 0..reads_before_throttling() {
            screen.next_update().unwrap();
        }
        assert_eq!(screen.next_update().unwrap().regions.len(), 0);
        screen.wake();
        assert_ne!(screen.next_update().unwrap().regions.len(), 0);
        let consumed = screen.grabber.0.len();
        screen.next_update().unwrap();
        assert_eq!(screen.grabber.0.len(), consumed - 1);
    }

    #[test]
    fn a_failed_grab_is_reported() {
        let mut screen = screen(Vec::new());
        assert!(matches!(
            screen.next_update(),
            Err(CaptureError::PermissionDenied)
        ));
    }
}
