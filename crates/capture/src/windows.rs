use std::sync::Arc;
use std::time::{Duration, Instant};

use dxgi_capture_rs::{CaptureError as DxgiError, DXGIManager};

use crate::arrange::{self, OutputRect, View};
use crate::bitmap::{Bitmap, Rect};
use crate::error::CaptureError;
use crate::polling::{Backoff, changed_regions, is_busy};
use crate::retry::{RETRY_INTERVAL, Retry};
use crate::shared::{Duplicator, Grab, Lease, Polled, Shared};
use crate::{Monitors, Region, ScreenSource, Update};

const FRAME_TIMEOUT_MS: u32 = 10;
const FIRST_FRAME_DEADLINE: Duration = Duration::from_secs(5);
const BYTES_PER_PIXEL: usize = 4;
const MAX_OUTPUTS: usize = 16;

pub type PrimarySize = Box<dyn Fn() -> Result<(u16, u16), CaptureError> + Send>;

enum Acquired {
    Idle,
    Frame { bitmap: Arc<Bitmap>, reset: bool },
}

struct Dxgi(DXGIManager);

impl Duplicator for Dxgi {
    fn grab(&mut self) -> Result<Grab, CaptureError> {
        match self.0.capture_frame_components() {
            Ok((pixels, (width, height))) => Ok(Grab::Frame(to_bitmap(pixels, width, height)?)),
            Err(DxgiError::Timeout) => Ok(Grab::Idle),
            Err(DxgiError::AccessLost | DxgiError::RefreshFailure | DxgiError::AccessDenied) => {
                Ok(Grab::Lost)
            }
            Err(DxgiError::Fail(error)) => Err(CaptureError::Request(error.to_string())),
        }
    }
}

static DISPLAYS: [Shared<Dxgi>; MAX_OUTPUTS] = [const { Shared::new() }; MAX_OUTPUTS];

struct Source {
    output: usize,
    lease: Lease<'static, Dxgi>,
    last: Option<Arc<Bitmap>>,
    next_try: Option<Instant>,
}

struct Display {
    primary_size: PrimarySize,
    primary: usize,
    rects: Vec<OutputRect>,
    view: View,
    sources: Vec<Source>,
    dirty: bool,
    retry: Retry,
}

impl Display {
    fn connect(primary_size: PrimarySize) -> Result<Self, CaptureError> {
        let rects = outputs();
        let primary = find_primary(&rects, &primary_size)?;
        let mut display = Self {
            primary_size,
            primary,
            rects,
            view: View::Output(primary),
            sources: Vec::new(),
            dirty: false,
            retry: Retry::default(),
        };
        display.open_view(View::Output(primary))?;
        Ok(display)
    }

    fn open_view(&mut self, view: View) -> Result<(), CaptureError> {
        let outputs: Vec<usize> = match view {
            View::Output(output) => vec![output],
            View::All => (0..self.rects.len()).collect(),
        };
        let mut sources = Vec::with_capacity(outputs.len());
        for output in outputs {
            let mut lease = DISPLAYS
                .get(output)
                .ok_or_else(|| CaptureError::Connect(format!("output {output} is not reachable")))?
                .lease();
            let expected = self.rects.get(output).copied();
            lease.ensure(|| open_output(output, expected))?;
            sources.push(Source {
                output,
                lease,
                last: None,
                next_try: None,
            });
        }
        self.sources = sources;
        self.view = view;
        self.dirty = false;
        self.retry = Retry::default();
        Ok(())
    }

    fn refresh_layout(&mut self) -> bool {
        let rects = outputs();
        if rects.is_empty() || rects == self.rects {
            return false;
        }
        self.rects = rects;
        if let Ok(primary) = find_primary(&self.rects, &self.primary_size) {
            self.primary = primary;
        }
        let count = self.rects.len();
        let target = match self.view {
            View::Output(output) if output < count => return false,
            View::All if count > 1 && self.sources.len() == count => return false,
            View::All if count > 1 => View::All,
            View::Output(_) | View::All => View::Output(self.primary),
        };
        self.open_view(target).is_ok() || self.open_view(View::Output(self.primary)).is_ok()
    }

    fn acquire(&mut self) -> Result<Acquired, CaptureError> {
        let now = Instant::now();
        if self.retry.waiting(now) {
            return Ok(Acquired::Idle);
        }
        let reset = self.retry.pending();
        let single = self.sources.len() == 1;
        let mut blocked = false;
        for source in &mut self.sources {
            if source.next_try.is_some_and(|at| now < at) {
                continue;
            }
            let output = source.output;
            let expected = self.rects.get(output).copied();
            match source.lease.poll(|| open_output(output, expected))? {
                Polled::Frame(bitmap) => {
                    source.last = Some(bitmap);
                    source.next_try = None;
                    self.dirty = true;
                }
                Polled::Idle => source.next_try = None,
                Polled::Lost | Polled::Unavailable if single => blocked = true,
                Polled::Lost | Polled::Unavailable => {
                    source.next_try = Some(now + RETRY_INTERVAL);
                    self.dirty = true;
                }
            }
        }
        if blocked {
            return Ok(self.blocked(now));
        }
        if !self.dirty {
            return Ok(Acquired::Idle);
        }
        let bitmap = self.picture()?;
        self.dirty = false;
        self.retry.recovered();
        Ok(Acquired::Frame { bitmap, reset })
    }

    fn picture(&self) -> Result<Arc<Bitmap>, CaptureError> {
        match self.view {
            View::Output(_) => self
                .sources
                .first()
                .and_then(|source| source.last.clone())
                .ok_or_else(|| CaptureError::Request("the display produced no frame".to_owned())),
            View::All => {
                let area = arrange::bounds(&self.rects)
                    .ok_or_else(|| CaptureError::Request("there are no outputs".to_owned()))?;
                let parts: Vec<(OutputRect, Option<&Bitmap>)> = self
                    .sources
                    .iter()
                    .filter_map(|source| {
                        Some((*self.rects.get(source.output)?, source.last.as_deref()))
                    })
                    .collect();
                arrange::compose(area, &parts).map(Arc::new)
            }
        }
    }

    fn blocked(&mut self, now: Instant) -> Acquired {
        self.retry.failed(now);
        Acquired::Idle
    }
}

fn outputs() -> Vec<OutputRect> {
    DXGIManager::output_rects()
}

fn find_primary(rects: &[OutputRect], primary_size: &PrimarySize) -> Result<usize, CaptureError> {
    let wanted = primary_size()?;
    arrange::primary_output(rects, wanted).ok_or_else(|| {
        CaptureError::Connect(format!(
            "no output matches the primary monitor of {}x{}; outputs found: {rects:?}",
            wanted.0, wanted.1
        ))
    })
}

fn open_output(output: usize, expected: Option<OutputRect>) -> Result<Dxgi, CaptureError> {
    let manager = DXGIManager::new_for_output(FRAME_TIMEOUT_MS, output)
        .map_err(|error| CaptureError::Connect(format!("{error:?}")))?;
    if let Some(rect) = expected {
        let wanted = (
            usize::try_from(rect.2 - rect.0).unwrap_or(0),
            usize::try_from(rect.3 - rect.1).unwrap_or(0),
        );
        if manager.geometry() != wanted {
            return Err(CaptureError::Connect(format!(
                "output {output} is not the monitor that was listed"
            )));
        }
    }
    Ok(Dxgi(manager))
}

pub struct WindowsScreen {
    display: Display,
    previous: Option<Arc<Bitmap>>,
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

    fn frame(
        &mut self,
        now: Instant,
        bitmap: Arc<Bitmap>,
        reset: bool,
    ) -> Result<Update, CaptureError> {
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
                bitmap: Bitmap::clone(&bitmap),
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

    fn monitors(&mut self) -> Monitors {
        if self.display.refresh_layout() {
            self.previous = None;
        }
        arrange::monitors(&self.display.rects, self.display.primary, self.display.view)
    }

    fn select_monitor(&mut self, index: u8) -> bool {
        let Some(view) = arrange::view_for(&self.display.rects, self.display.primary, index) else {
            return false;
        };
        if view == self.display.view || self.display.open_view(view).is_err() {
            return false;
        }
        self.previous = None;
        self.backoff.wake();
        true
    }
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
