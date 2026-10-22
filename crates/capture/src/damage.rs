use crate::bitmap::{Bitmap, Rect};

pub const TILE_SIZE: u16 = 64;

#[must_use]
pub fn changed_tiles(previous: &Bitmap, current: &Bitmap) -> Vec<Rect> {
    if previous.width() != current.width() || previous.height() != current.height() {
        return vec![whole(current)];
    }
    let mut tiles = Vec::new();
    for y in (0..current.height()).step_by(usize::from(TILE_SIZE)) {
        for x in (0..current.width()).step_by(usize::from(TILE_SIZE)) {
            let tile = tile_at(current, x, y);
            if tile_differs(previous, current, tile) {
                tiles.push(tile);
            }
        }
    }
    tiles
}

fn whole(bitmap: &Bitmap) -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: bitmap.width(),
        height: bitmap.height(),
    }
}

fn tile_at(bitmap: &Bitmap, x: u16, y: u16) -> Rect {
    Rect {
        x,
        y,
        width: TILE_SIZE.min(bitmap.width() - x),
        height: TILE_SIZE.min(bitmap.height() - y),
    }
}

fn tile_differs(previous: &Bitmap, current: &Bitmap, tile: Rect) -> bool {
    let left = usize::from(tile.x) * 4;
    let right = left + usize::from(tile.width) * 4;
    (tile.y..tile.y + tile.height)
        .any(|y| previous.row(y)[left..right] != current.row(y)[left..right])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(width: u16, height: u16) -> Bitmap {
        Bitmap::new(
            width,
            height,
            vec![0; usize::from(width) * usize::from(height) * 4],
        )
        .unwrap()
    }

    fn with_pixel(width: u16, height: u16, x: u16, y: u16) -> Bitmap {
        let mut data = vec![0; usize::from(width) * usize::from(height) * 4];
        let at = (usize::from(y) * usize::from(width) + usize::from(x)) * 4;
        data[at] = 255;
        Bitmap::new(width, height, data).unwrap()
    }

    #[test]
    fn identical_frames_have_no_changes() {
        assert_eq!(
            changed_tiles(&blank(200, 130), &blank(200, 130)),
            Vec::<Rect>::new()
        );
    }

    #[test]
    fn one_pixel_marks_only_its_tile() {
        let tiles = changed_tiles(&blank(200, 130), &with_pixel(200, 130, 70, 5));
        assert_eq!(
            tiles,
            [Rect {
                x: 64,
                y: 0,
                width: 64,
                height: 64
            }]
        );
    }

    #[test]
    fn edge_tiles_are_clipped_to_the_screen() {
        let tiles = changed_tiles(&blank(200, 130), &with_pixel(200, 130, 199, 129));
        assert_eq!(
            tiles,
            [Rect {
                x: 192,
                y: 128,
                width: 8,
                height: 2
            }]
        );
    }

    #[test]
    fn a_resize_replaces_the_whole_screen() {
        let tiles = changed_tiles(&blank(100, 100), &blank(120, 100));
        assert_eq!(
            tiles,
            [Rect {
                x: 0,
                y: 0,
                width: 120,
                height: 100
            }]
        );
    }
}
