use qrcode::{Color, EcLevel, QrCode};

const QUIET: usize = 3;

#[must_use]
pub fn render(data: &str) -> Option<String> {
    render_with(data, !cfg!(windows))
}

fn render_with(data: &str, ansi: bool) -> Option<String> {
    let code = QrCode::with_error_correction_level(data, EcLevel::M).ok()?;
    let width = code.width();
    let colors = code.to_colors();
    let side = width + 2 * QUIET;
    let dark = |x: usize, y: usize| {
        x >= QUIET
            && y >= QUIET
            && x < QUIET + width
            && y < QUIET + width
            && colors[(y - QUIET) * width + (x - QUIET)] == Color::Dark
    };
    let mut out = String::new();
    let mut y = 0;
    while y < side {
        if ansi {
            out.push_str("\x1b[30;47m");
        }
        for x in 0..side {
            let (top, bottom) = (dark(x, y), dark(x, y + 1));
            out.push(match (top, bottom, ansi) {
                (false, false, true) | (true, true, false) => ' ',
                (true, false, true) | (false, true, false) => '\u{2580}',
                (false, true, true) | (true, false, false) => '\u{2584}',
                (true, true, true) | (false, false, false) => '\u{2588}',
            });
        }
        if ansi {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
        y += 2;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_text_renders_as_a_square_of_blocks() {
        let text = render("otpauth://totp/Wirehaus:test?secret=ABCDEFGH").unwrap();
        let rows: Vec<&str> = text.lines().collect();
        assert!(rows.len() >= 12);
        assert!(
            rows.iter()
                .all(|row| row.starts_with("\x1b[30;47m") && row.ends_with("\x1b[0m"))
        );
        assert!(text.contains('\u{2588}'));
    }

    #[test]
    fn every_row_has_the_same_width() {
        let text = render("https://app.wirehaus.dev/zz-test#pair=ABCD-EFGH").unwrap();
        let widths: Vec<usize> = text
            .lines()
            .map(|row| {
                row.replace("\x1b[30;47m", "")
                    .replace("\x1b[0m", "")
                    .chars()
                    .count()
            })
            .collect();
        assert!(widths.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn the_plain_variant_has_no_escape_codes_and_inverts_the_blocks() {
        let data = "otpauth://totp/Wirehaus:test?secret=ABCDEFGH";
        let plain = render_with(data, false).unwrap();
        let coloured = render_with(data, true).unwrap();
        assert!(!plain.contains('\x1b'));
        assert_eq!(plain.lines().count(), coloured.lines().count());
        assert!(plain.starts_with('\u{2588}'));
    }

    #[test]
    fn text_that_cannot_fit_gives_nothing() {
        assert!(render(&"x".repeat(5000)).is_none());
    }
}
