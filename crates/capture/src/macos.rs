use core_graphics::access::ScreenCaptureAccess;
use core_graphics::display::CGDisplay;
use core_graphics::image::CGImage;

use crate::bitmap::Bitmap;
use crate::error::CaptureError;
use crate::polling::Grabber;

const BYTES_PER_PIXEL: usize = 4;
const COMPONENT_BITS: usize = 8;

#[must_use]
pub fn screen_capture_allowed() -> bool {
    ScreenCaptureAccess.preflight()
}

#[must_use]
pub fn request_screen_capture() -> bool {
    ScreenCaptureAccess.request()
}

#[derive(Debug)]
pub struct MacDisplay;

impl MacDisplay {
    pub fn connect() -> Result<Self, CaptureError> {
        if !screen_capture_allowed() {
            return Err(CaptureError::PermissionDenied);
        }
        Ok(Self)
    }
}

impl Grabber for MacDisplay {
    fn grab(&mut self) -> Result<Bitmap, CaptureError> {
        let image = CGDisplay::main()
            .image()
            .ok_or_else(|| CaptureError::Request("the display returned no image".to_owned()))?;
        to_bitmap(&image)
    }
}

fn to_bitmap(image: &CGImage) -> Result<Bitmap, CaptureError> {
    let bits = image.bits_per_pixel();
    if bits != BYTES_PER_PIXEL * 8 || image.bits_per_component() != COMPONENT_BITS {
        return Err(CaptureError::UnsupportedFormat {
            depth: 24,
            bits_per_pixel: u8::try_from(bits).unwrap_or(u8::MAX),
        });
    }
    let (width, height) = (image.width(), image.height());
    let size_error = || CaptureError::InvalidSize {
        width: u16::try_from(width).unwrap_or(u16::MAX),
        height: u16::try_from(height).unwrap_or(u16::MAX),
    };
    let (columns, rows) = (
        u16::try_from(width).map_err(|_| size_error())?,
        u16::try_from(height).map_err(|_| size_error())?,
    );
    let data = image.data();
    let bgra = pack_rows(&data, image.bytes_per_row(), width, height)?;
    Bitmap::new(columns, rows, bgra)
}

fn pack_rows(
    source: &[u8],
    stride: usize,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, CaptureError> {
    let row = width * BYTES_PER_PIXEL;
    let expected = stride * height.saturating_sub(1) + row;
    if stride < row || source.len() < expected {
        return Err(CaptureError::ShortImage {
            expected,
            actual: source.len(),
        });
    }
    let mut out = Vec::with_capacity(row * height);
    for line in 0..height {
        let start = line * stride;
        out.extend_from_slice(&source[start..start + row]);
    }
    for pixel in out.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
        pixel[3] = 255;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_row_padding_and_forces_opaque_pixels() {
        let source = [
            1, 2, 3, 0, 4, 5, 6, 0, 99, 99, 99, 99, 7, 8, 9, 0, 10, 11, 12, 0, 99, 99, 99, 99,
        ];
        let packed = pack_rows(&source, 12, 2, 2).unwrap();
        assert_eq!(
            packed,
            [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
        );
    }

    #[test]
    fn rejects_data_shorter_than_the_image() {
        assert!(matches!(
            pack_rows(&[0; 10], 8, 2, 2),
            Err(CaptureError::ShortImage { .. })
        ));
        assert!(matches!(
            pack_rows(&[0; 64], 4, 2, 2),
            Err(CaptureError::ShortImage { .. })
        ));
    }
}
