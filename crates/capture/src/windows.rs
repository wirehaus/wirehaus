use dxgi_capture_rs::{CaptureError as DxgiError, DXGIManager};

use crate::bitmap::Bitmap;
use crate::error::CaptureError;
use crate::polling::Grabber;

const FRAME_TIMEOUT_MS: u32 = 10;
const BYTES_PER_PIXEL: usize = 4;

pub struct WindowsDisplay {
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
    pub fn connect() -> Result<Self, CaptureError> {
        Ok(Self {
            manager: open()?,
            last: None,
        })
    }

    fn reconnect(&mut self) -> Result<(), CaptureError> {
        self.manager = open()?;
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

fn open() -> Result<DXGIManager, CaptureError> {
    DXGIManager::new(FRAME_TIMEOUT_MS).map_err(|error| CaptureError::Connect(format!("{error:?}")))
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
