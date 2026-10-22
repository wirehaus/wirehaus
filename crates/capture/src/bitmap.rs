use crate::error::CaptureError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    width: u16,
    height: u16,
    bgra: Vec<u8>,
}

impl Bitmap {
    pub fn new(width: u16, height: u16, bgra: Vec<u8>) -> Result<Self, CaptureError> {
        let expected = byte_len(width, height);
        if width == 0 || height == 0 {
            return Err(CaptureError::InvalidSize { width, height });
        }
        if bgra.len() != expected {
            return Err(CaptureError::ShortImage {
                expected,
                actual: bgra.len(),
            });
        }
        Ok(Self {
            width,
            height,
            bgra,
        })
    }

    #[must_use]
    pub fn width(&self) -> u16 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u16 {
        self.height
    }

    #[must_use]
    pub fn bgra(&self) -> &[u8] {
        &self.bgra
    }

    #[must_use]
    pub fn row(&self, y: u16) -> &[u8] {
        let stride = usize::from(self.width) * 4;
        let start = usize::from(y) * stride;
        &self.bgra[start..start + stride]
    }

    #[must_use]
    pub fn crop(&self, rect: Rect) -> Vec<u8> {
        let mut out = Vec::with_capacity(byte_len(rect.width, rect.height));
        let left = usize::from(rect.x) * 4;
        let right = left + usize::from(rect.width) * 4;
        for y in rect.y..rect.y + rect.height {
            out.extend_from_slice(&self.row(y)[left..right]);
        }
        out
    }
}

fn byte_len(width: u16, height: u16) -> usize {
    usize::from(width) * usize::from(height) * 4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u16, height: u16) -> Bitmap {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[
                    u8::try_from(x % 256).unwrap(),
                    u8::try_from(y % 256).unwrap(),
                    0,
                    255,
                ]);
            }
        }
        Bitmap::new(width, height, data).unwrap()
    }

    #[test]
    fn rejects_a_buffer_of_the_wrong_length() {
        assert!(matches!(
            Bitmap::new(2, 2, vec![0; 15]),
            Err(CaptureError::ShortImage {
                expected: 16,
                actual: 15
            })
        ));
    }

    #[test]
    fn rejects_an_empty_bitmap() {
        assert!(matches!(
            Bitmap::new(0, 4, vec![]),
            Err(CaptureError::InvalidSize { .. })
        ));
    }

    #[test]
    fn crops_a_region() {
        let bitmap = gradient(4, 4);
        let part = bitmap.crop(Rect {
            x: 1,
            y: 2,
            width: 2,
            height: 2,
        });
        assert_eq!(
            part,
            [1, 2, 0, 255, 2, 2, 0, 255, 1, 3, 0, 255, 2, 3, 0, 255]
        );
    }
}
