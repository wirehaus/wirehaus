use std::time::{Duration, Instant};

use dxgi_capture_rs::{CaptureError as DxgiError, DXGIManager};

use crate::bitmap::{Bitmap, Rect};
use crate::error::CaptureError;
use crate::polling::{Backoff, changed_regions, is_busy};
use crate::retry::Retry;
use crate::{Region, ScreenSource, Update};

const FRAME_TIMEOUT_MS: u32 = 10;
const FIRST_FRAME_DEADLINE: Duration = Duration::from_secs(5);
const BYTES_PER_PIXEL: usize = 4;
const MAX_OUTPUTS: usize = 16;

pub type PrimarySize = Box<dyn Fn() -> Result<(u16, u16), CaptureError> + Send>;

enum Acquired {
    Idle,
    Frame { bitmap: Bitmap, reset: bool },
}

struct Display {
    primary: PrimarySize,
    manager: Option<DXGIManager>,
    retry: Retry,
}

impl Display {
    fn connect(primary: PrimarySize) -> Result<Self, CaptureError> {
        let manager = open(&primary)?;
        Ok(Self {
            primary,
            manager: Some(manager),
            retry: Retry::default(),
        })
    }

    fn acquire(&mut self) -> Result<Acquired, CaptureError> {
        let now = Instant::now();
        if self.retry.waiting(now) {
            return Ok(Acquired::Idle);
        }
        let reset = self.retry.pending();
        if self.manager.is_none() {
            match open(&self.primary) {
                Ok(manager) => self.manager = Some(manager),
                Err(_) => return Ok(self.blocked(now)),
            }
        }
        let Some(manager) = self.manager.as_mut() else {
            return Ok(self.blocked(now));
        };
        match manager.capture_frame_components() {
            Ok((pixels, (width, height))) => {
                self.retry.recovered();
                Ok(Acquired::Frame {
                    bitmap: to_bitmap(pixels, width, height)?,
                    reset,
                })
            }
            Err(DxgiError::Timeout) => Ok(Acquired::Idle),
            Err(DxgiError::AccessLost | DxgiError::RefreshFailure | DxgiError::AccessDenied) => {
                self.manager = None;
                Ok(self.blocked(now))
            }
            Err(DxgiError::Fail(error)) => Err(CaptureError::Request(error.to_string())),
        }
    }

    fn blocked(&mut self, now: Instant) -> Acquired {
        self.retry.failed(now);
        Acquired::Idle
    }
}

pub struct WindowsScreen {
    display: Display,
    previous: Option<Bitmap>,
    backoff: Backoff,
}

impl std::fmt::Debug for WindowsScreen {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsScreen")
            .field(
                "size",
                &self
                    .previous
                    .as_ref()
                    .map(|frame| (frame.width(), frame.height())),
            )
            .finish_non_exhaustive()
    }
}

impl WindowsScreen {
    pub fn connect(primary: PrimarySize) -> Result<Self, CaptureError> {
        Ok(Self {
            display: Display::connect(primary)?,
            previous: None,
            backoff: Backoff::default(),
        })
    }

    fn idle(&self) -> Option<Update> {
        self.previous.as_ref().map(|frame| Update {
            width: frame.width(),
            height: frame.height(),
            resized: false,
            regions: Vec::new(),
        })
    }

    fn frame(&mut self, now: Instant, bitmap: Bitmap, reset: bool) -> Result<Update, CaptureError> {
        let (width, height) = (bitmap.width(), bitmap.height());
        let same_size = self
            .previous
            .as_ref()
            .is_some_and(|previous| previous.width() == width && previous.height() == height);
        let full = !same_size || reset;
        let regions = match (&self.previous, full) {
            (Some(previous), false) => changed_regions(previous, &bitmap)?,
            _ => vec![Region {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
                bitmap: bitmap.clone(),
            }],
        };
        self.backoff
            .observe(now, full || is_busy(&regions, width, height));
        self.previous = Some(bitmap);
        Ok(Update {
            width,
            height,
            resized: !same_size,
            regions,
        })
    }
}

impl ScreenSource for WindowsScreen {
    fn next_update(&mut self) -> Result<Update, CaptureError> {
        let started = Instant::now();
        if !self.backoff.due(started)
            && let Some(update) = self.idle()
        {
            return Ok(update);
        }
        loop {
            let now = Instant::now();
            match self.display.acquire()? {
                Acquired::Frame { bitmap, reset } => return self.frame(now, bitmap, reset),
                Acquired::Idle => {
                    self.backoff.observe(now, false);
                    if let Some(update) = self.idle() {
                        return Ok(update);
                    }
                    if started.elapsed() > FIRST_FRAME_DEADLINE {
                        return Err(CaptureError::Request(
                            "the display produced no frame".to_owned(),
                        ));
                    }
                }
            }
        }
    }

    fn wake(&mut self) {
        self.backoff.wake();
    }
}

fn open(primary: &PrimarySize) -> Result<DXGIManager, CaptureError> {
    let wanted = primary()?;
    let mut manager = DXGIManager::new(FRAME_TIMEOUT_MS)
        .map_err(|error| CaptureError::Connect(format!("{error:?}")))?;
    let mut sizes = Vec::new();
    for index in 0..MAX_OUTPUTS {
        manager.set_capture_source_index(index);
        let (width, height) = manager.geometry();
        if width == 0 || height == 0 {
            break;
        }
        sizes.push((width, height));
    }
    let index = select_output(&sizes, wanted).ok_or_else(|| {
        CaptureError::Connect(format!(
            "no output matches the primary monitor of {}x{}; outputs found: {sizes:?}",
            wanted.0, wanted.1
        ))
    })?;
    manager.set_capture_source_index(index);
    Ok(manager)
}

fn select_output(sizes: &[(usize, usize)], primary: (u16, u16)) -> Option<usize> {
    let (width, height) = (usize::from(primary.0), usize::from(primary.1));
    sizes
        .iter()
        .position(|&size| size == (width, height))
        .or_else(|| sizes.iter().position(|&size| size == (height, width)))
}

fn to_bitmap(mut pixels: Vec<u8>, width: usize, height: usize) -> Result<Bitmap, CaptureError> {
    let expected = width * height * BYTES_PER_PIXEL;
    if pixels.len() != expected {
        return Err(CaptureError::ShortImage {
            expected,
            actual: pixels.len(),
        });
    }
    let size_error = || CaptureError::InvalidSize {
        width: u16::try_from(width).unwrap_or(u16::MAX),
        height: u16::try_from(height).unwrap_or(u16::MAX),
    };
    let (columns, rows) = (
        u16::try_from(width).map_err(|_| size_error())?,
        u16::try_from(height).map_err(|_| size_error())?,
    );
    for pixel in pixels.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
        pixel[3] = 255;
    }
    Bitmap::new(columns, rows, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_output_with_the_size_of_the_primary_monitor() {
        let sizes = [(1280, 1024), (1920, 1080), (1920, 1080)];
        assert_eq!(select_output(&sizes, (1920, 1080)), Some(1));
        assert_eq!(select_output(&sizes, (1280, 1024)), Some(0));
    }

    #[test]
    fn a_rotated_primary_matches_its_unrotated_output() {
        assert_eq!(select_output(&[(1920, 1080)], (1080, 1920)), Some(0));
    }

    #[test]
    fn no_matching_output_is_not_guessed() {
        assert_eq!(select_output(&[(1920, 1080)], (2560, 1440)), None);
        assert_eq!(select_output(&[], (1920, 1080)), None);
    }

    #[test]
    fn forces_opaque_pixels() {
        let bitmap = to_bitmap(vec![1, 2, 3, 0, 4, 5, 6, 7], 2, 1).unwrap();
        assert_eq!(bitmap.width(), 2);
    }

    #[test]
    fn rejects_a_buffer_of_the_wrong_length() {
        assert!(matches!(
            to_bitmap(vec![0; 7], 2, 1),
            Err(CaptureError::ShortImage { .. })
        ));
    }
}
