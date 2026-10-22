use crate::bitmap::{Bitmap, Rect};
use crate::damage::{changed_tiles, coalesce};
use crate::error::CaptureError;
use crate::{Region, ScreenSource, Update};

pub trait Grabber {
    fn grab(&mut self) -> Result<Bitmap, CaptureError>;
}

#[derive(Debug)]
pub struct PollingScreen<G> {
    grabber: G,
    previous: Option<Bitmap>,
}

impl<G: Grabber> PollingScreen<G> {
    pub fn new(grabber: G) -> Self {
        Self {
            grabber,
            previous: None,
        }
    }
}

impl<G: Grabber> ScreenSource for PollingScreen<G> {
    fn next_update(&mut self) -> Result<Update, CaptureError> {
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
        self.previous = Some(current);
        Ok(Update {
            width,
            height,
            resized: !same_size,
            regions,
        })
    }
}

fn changed_regions(previous: &Bitmap, current: &Bitmap) -> Result<Vec<Region>, CaptureError> {
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
    fn a_failed_grab_is_reported() {
        let mut screen = screen(Vec::new());
        assert!(matches!(
            screen.next_update(),
            Err(CaptureError::PermissionDenied)
        ));
    }
}
