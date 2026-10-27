use qrcode::{Color, EcLevel, QrCode};

const MAX_TEXT: usize = 600;
const MAX_SIDE: usize = 4096;

pub fn matrix(text: &str) -> Result<Vec<u8>, String> {
    if text.is_empty() || text.len() > MAX_TEXT {
        return Err("the text does not fit in a QR code".to_owned());
    }
    let code = QrCode::with_error_correction_level(text, EcLevel::M).map_err(|e| e.to_string())?;
    let width = code.width();
    let side = u8::try_from(width).map_err(|_| "the QR code is too large".to_owned())?;
    let mut out = Vec::with_capacity(1 + width * width);
    out.push(side);
    out.extend(
        code.to_colors()
            .into_iter()
            .map(|c| u8::from(c == Color::Dark)),
    );
    Ok(out)
}

pub fn decode(width: usize, height: usize, luma: &[u8]) -> Option<String> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    if luma.len() != width.checked_mul(height)? {
        return None;
    }
    let mut image =
        rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| luma[y * width + x]);
    image
        .detect_grids()
        .into_iter()
        .find_map(|grid| grid.decode().ok().map(|(_, text)| text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(text: &str, scale: usize) -> (usize, usize, Vec<u8>) {
        let data = matrix(text).unwrap();
        let modules = usize::from(data[0]);
        let quiet = 4;
        let size = (modules + 2 * quiet) * scale;
        let mut luma = vec![255_u8; size * size];
        for y in 0..modules {
            for x in 0..modules {
                if data[1 + y * modules + x] == 1 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            luma[((y + quiet) * scale + dy) * size + (x + quiet) * scale + dx] = 0;
                        }
                    }
                }
            }
        }
        (size, size, luma)
    }

    #[test]
    fn what_is_drawn_can_be_read_back() {
        let link = "https://app.wirehaus.dev/zz-test#link=0123456789abcdef0123456789abcdef";
        let (w, h, luma) = render(link, 5);
        assert_eq!(decode(w, h, &luma).as_deref(), Some(link));
    }

    #[test]
    fn the_same_text_gives_the_same_matrix_and_big_texts_are_refused() {
        assert_eq!(matrix("abc").unwrap(), matrix("abc").unwrap());
        assert!(matrix("").is_err());
        assert!(matrix(&"x".repeat(MAX_TEXT + 1)).is_err());
    }

    #[test]
    fn noise_and_wrong_sizes_decode_to_nothing() {
        assert_eq!(decode(10, 10, &[128; 100]), None);
        assert_eq!(decode(10, 10, &[0; 5]), None);
        assert_eq!(decode(0, 0, &[]), None);
        assert_eq!(decode(5000, 5000, &[]), None);
    }
}
