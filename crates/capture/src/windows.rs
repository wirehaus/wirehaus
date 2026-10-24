use dxgi_capture_rs::{CaptureError as DxgiError, DXGIManager};

use crate::bitmap::Bitmap;
use crate::error::CaptureError;
use crate::polling::Grabber;

const FRAME_TIMEOUT_MS: u32 = 10;
const BYTES_PER_PIXEL: usize = 4;

const MAX_OUTPUTS: usize = 16;

pub type PrimarySize = Box<dyn Fn() -> Result<(u16, u16), CaptureError> + Send>;

pub struct WindowsDisplay {
    primary: PrimarySize,
    manager: DXGIManager,
    last: Option<Bitmap>,
}

impl std::fmt::Debug for WindowsDisplay {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsDisplay")
            .finish_non_exhaustive()
    }
}

impl WindowsDisplay {
    pub fn connect(primary: PrimarySize) -> Result<Self, CaptureError> {
        let manager = open(&primary)?;
        Ok(Self {
            primary,
            manager,
            last: None,
        })
    }

    fn reconnect(&mut self) -> Result<(), CaptureError> {
        self.manager = open(&self.primary)?;
        Ok(())
    }
}

impl Grabber for WindowsDisplay {
    fn grab(&mut self) -> Result<Bitmap, CaptureError> {
        match self.manager.capture_frame_components() {
            Ok((pixels, (width, height))) => {
                let bitmap = to_bitmap(pixels, width, height)?;
                self.last = Some(bitmap.clone());
                Ok(bitmap)
            }
            Err(DxgiError::Timeout) => self.last.clone().map_or_else(|| self.grab_again(), Ok),
            Err(DxgiError::AccessLost | DxgiError::RefreshFailure) => {
                self.reconnect()?;
                self.grab_again()
            }
            Err(DxgiError::AccessDenied) => Err(CaptureError::PermissionDenied),
            Err(DxgiError::Fail(error)) => Err(CaptureError::Request(error.to_string())),
        }
    }
}

impl WindowsDisplay {
    fn grab_again(&mut self) -> Result<Bitmap, CaptureError> {
        match self.manager.capture_frame_components() {
            Ok((pixels, (width, height))) => {
                let bitmap = to_bitmap(pixels, width, height)?;
                self.last = Some(bitmap.clone());
                Ok(bitmap)
            }
            Err(DxgiError::Timeout) => self
                .last
                .clone()
                .ok_or_else(|| CaptureError::Request("the display produced no frame".to_owned())),
            Err(error) => Err(CaptureError::Request(error.to_string())),
        }
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
