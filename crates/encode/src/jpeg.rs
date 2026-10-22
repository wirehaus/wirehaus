use jpeg_encoder::{ColorType, Encoder};
use wirehaus_capture::{Bitmap, Rect};
use wirehaus_protocol::Codec;

use crate::TileEncoder;
use crate::error::EncodeError;

const DEFAULT_QUALITY: u8 = 70;

#[derive(Debug, Clone, Copy)]
pub struct JpegEncoder {
    quality: u8,
}

impl JpegEncoder {
    #[must_use]
    pub fn new(quality: u8) -> Self {
        Self {
            quality: quality.clamp(1, 100),
        }
    }
}

impl Default for JpegEncoder {
    fn default() -> Self {
        Self::new(DEFAULT_QUALITY)
    }
}

impl TileEncoder for JpegEncoder {
    fn codec(&self) -> Codec {
        Codec::Jpeg
    }

    fn encode(&mut self, bitmap: &Bitmap, rect: Rect) -> Result<Vec<u8>, EncodeError> {
        ensure_inside(bitmap, rect)?;
        let pixels = bitmap.crop(rect);
        let mut out = Vec::new();
        Encoder::new(&mut out, self.quality)
            .encode(&pixels, rect.width, rect.height, ColorType::Bgra)
            .map_err(|e| EncodeError::Encoder(e.to_string()))?;
        Ok(out)
    }
}

fn ensure_inside(bitmap: &Bitmap, rect: Rect) -> Result<(), EncodeError> {
    let fits = rect.width > 0
        && rect.height > 0
        && u32::from(rect.x) + u32::from(rect.width) <= u32::from(bitmap.width())
        && u32::from(rect.y) + u32::from(rect.height) <= u32::from(bitmap.height());
    if fits {
        Ok(())
    } else {
        Err(EncodeError::OutOfBounds {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            screen_width: bitmap.width(),
            screen_height: bitmap.height(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u16, height: u16, bgra: [u8; 4]) -> Bitmap {
        let data = bgra
            .iter()
            .copied()
            .cycle()
            .take(usize::from(width) * usize::from(height) * 4)
            .collect();
        Bitmap::new(width, height, data).unwrap()
    }

    fn dimensions(jpeg: &[u8]) -> (u16, u16) {
        let mut at = 2;
        while at + 9 < jpeg.len() {
            let marker = jpeg[at + 1];
            let length = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
            if marker == 0xC0 || marker == 0xC2 {
                let height = u16::from_be_bytes([jpeg[at + 5], jpeg[at + 6]]);
                let width = u16::from_be_bytes([jpeg[at + 7], jpeg[at + 8]]);
                return (width, height);
            }
            at += 2 + length;
        }
        panic!("no frame header found");
    }

    #[test]
    fn produces_a_jpeg_of_the_requested_size() {
        let bitmap = solid(300, 200, [10, 120, 200, 255]);
        let rect = Rect {
            x: 20,
            y: 30,
            width: 130,
            height: 64,
        };
        let jpeg = JpegEncoder::default().encode(&bitmap, rect).unwrap();
        assert_eq!(&jpeg[..2], [0xFF, 0xD8]);
        assert_eq!(&jpeg[jpeg.len() - 2..], [0xFF, 0xD9]);
        assert_eq!(dimensions(&jpeg), (130, 64));
    }

    #[test]
    fn a_flat_region_compresses_far_below_its_raw_size() {
        let bitmap = solid(512, 64, [0, 0, 0, 255]);
        let rect = Rect {
            x: 0,
            y: 0,
            width: 512,
            height: 64,
        };
        let jpeg = JpegEncoder::default().encode(&bitmap, rect).unwrap();
        assert!(jpeg.len() * 50 < 512 * 64 * 4);
    }

    #[test]
    fn rejects_a_region_outside_the_bitmap() {
        let bitmap = solid(100, 100, [0; 4]);
        let rect = Rect {
            x: 90,
            y: 0,
            width: 20,
            height: 10,
        };
        assert!(matches!(
            JpegEncoder::default().encode(&bitmap, rect),
            Err(EncodeError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn rejects_an_empty_region() {
        let bitmap = solid(100, 100, [0; 4]);
        let rect = Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 10,
        };
        assert!(matches!(
            JpegEncoder::default().encode(&bitmap, rect),
            Err(EncodeError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn keeps_the_quality_in_range() {
        assert_eq!(JpegEncoder::new(0).quality, 1);
        assert_eq!(JpegEncoder::new(250).quality, 100);
    }
}
