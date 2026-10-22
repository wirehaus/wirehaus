use crate::Update;
use crate::bitmap::Bitmap;
use crate::error::CaptureError;

#[derive(Debug, Default)]
pub struct FrameBuffer {
    frame: Option<Bitmap>,
}

impl FrameBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn frame(&self) -> Option<&Bitmap> {
        self.frame.as_ref()
    }

    pub fn apply(&mut self, update: &Update) -> Result<(), CaptureError> {
        let whole = update.regions.iter().find(|region| {
            region.rect.x == 0
                && region.rect.y == 0
                && region.rect.width == update.width
                && region.rect.height == update.height
        });
        if let Some(region) = whole {
            self.frame = Some(region.bitmap.clone());
            return Ok(());
        }
        let Some(frame) = self.frame.as_mut() else {
            return if update.regions.is_empty() {
                Ok(())
            } else {
                Err(CaptureError::InvalidSize {
                    width: update.width,
                    height: update.height,
                })
            };
        };
        if frame.width() != update.width || frame.height() != update.height {
            return Err(CaptureError::InvalidSize {
                width: update.width,
                height: update.height,
            });
        }
        for region in &update.regions {
            frame.paste(region.rect.x, region.rect.y, &region.bitmap)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rect, Region};

    fn solid(width: u16, height: u16, value: u8) -> Bitmap {
        Bitmap::new(
            width,
            height,
            vec![value; usize::from(width) * usize::from(height) * 4],
        )
        .unwrap()
    }

    fn whole(width: u16, height: u16, value: u8) -> Update {
        Update {
            width,
            height,
            resized: true,
            regions: vec![Region {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
                bitmap: solid(width, height, value),
            }],
        }
    }

    fn patch(width: u16, height: u16, x: u16, y: u16, w: u16, h: u16, value: u8) -> Update {
        Update {
            width,
            height,
            resized: false,
            regions: vec![Region {
                rect: Rect {
                    x,
                    y,
                    width: w,
                    height: h,
                },
                bitmap: solid(w, h, value),
            }],
        }
    }

    #[test]
    fn starts_empty_and_takes_the_first_whole_frame() {
        let mut buffer = FrameBuffer::new();
        assert!(buffer.frame().is_none());
        buffer.apply(&whole(8, 8, 1)).unwrap();
        assert_eq!(buffer.frame().unwrap().bgra(), solid(8, 8, 1).bgra());
    }

    #[test]
    fn partial_updates_change_only_their_region() {
        let mut buffer = FrameBuffer::new();
        buffer.apply(&whole(8, 8, 1)).unwrap();
        buffer.apply(&patch(8, 8, 2, 2, 3, 3, 7)).unwrap();
        let frame = buffer.frame().unwrap();
        assert_eq!(frame.row(0), [1; 32]);
        assert_eq!(&frame.row(3)[8..20], [7; 12]);
        assert_eq!(&frame.row(3)[..8], [1; 8]);
        assert_eq!(frame.row(5), [1; 32]);
    }

    #[test]
    fn an_empty_update_changes_nothing() {
        let mut buffer = FrameBuffer::new();
        buffer.apply(&whole(4, 4, 5)).unwrap();
        let empty = Update {
            width: 4,
            height: 4,
            resized: false,
            regions: vec![],
        };
        buffer.apply(&empty).unwrap();
        assert_eq!(buffer.frame().unwrap().bgra(), solid(4, 4, 5).bgra());
    }

    #[test]
    fn a_new_whole_frame_replaces_the_buffer_after_a_resize() {
        let mut buffer = FrameBuffer::new();
        buffer.apply(&whole(4, 4, 1)).unwrap();
        buffer.apply(&whole(6, 2, 2)).unwrap();
        let frame = buffer.frame().unwrap();
        assert_eq!((frame.width(), frame.height()), (6, 2));
    }

    #[test]
    fn a_partial_update_before_any_frame_is_an_error() {
        let mut buffer = FrameBuffer::new();
        assert!(buffer.apply(&patch(8, 8, 0, 0, 2, 2, 1)).is_err());
    }

    #[test]
    fn a_partial_update_for_a_different_size_is_an_error() {
        let mut buffer = FrameBuffer::new();
        buffer.apply(&whole(8, 8, 1)).unwrap();
        assert!(buffer.apply(&patch(10, 10, 0, 0, 2, 2, 1)).is_err());
    }
}
