use crate::bitmap::{Bitmap, Rect};

pub const TILE_SIZE: u16 = 64;
pub const MAX_RUN_WIDTH: u16 = 2048;

#[must_use]
pub fn changed_tiles(previous: &Bitmap, current: &Bitmap) -> Vec<Rect> {
    if previous.width() != current.width() || previous.height() != current.height() {
        return all_tiles(current);
    }
    tiles_where(current.width(), current.height(), |tile| {
        tile_differs(previous, current, tile)
    })
}

#[must_use]
pub fn all_tiles(bitmap: &Bitmap) -> Vec<Rect> {
    tiles_where(bitmap.width(), bitmap.height(), |_| true)
}

#[must_use]
pub fn tiles_touching(rects: &[Rect], width: u16, height: u16) -> Vec<Rect> {
    let columns = tile_count(width);
    let rows = tile_count(height);
    let mut dirty = vec![false; columns * rows];
    for rect in rects {
        let right = u32::from(rect.x) + u32::from(rect.width);
        let bottom = u32::from(rect.y) + u32::from(rect.height);
        if rect.width == 0 || rect.height == 0 || rect.x >= width || rect.y >= height {
            continue;
        }
        let first_column = usize::from(rect.x / TILE_SIZE);
        let first_row = usize::from(rect.y / TILE_SIZE);
        let last_column = last_tile(right, width);
        let last_row = last_tile(bottom, height);
        for row in first_row..=last_row {
            for column in first_column..=last_column {
                dirty[row * columns + column] = true;
            }
        }
    }
    tiles_where(width, height, |tile| {
        dirty[usize::from(tile.y / TILE_SIZE) * columns + usize::from(tile.x / TILE_SIZE)]
    })
}

#[must_use]
pub fn tile_count(length: u16) -> usize {
    usize::from(length).div_ceil(usize::from(TILE_SIZE))
}

fn last_tile(edge: u32, length: u16) -> usize {
    let clipped = edge.min(u32::from(length)).saturating_sub(1);
    usize::try_from(clipped / u32::from(TILE_SIZE)).unwrap_or(0)
}

fn tiles_where(width: u16, height: u16, mut keep: impl FnMut(Rect) -> bool) -> Vec<Rect> {
    let mut tiles = Vec::new();
    for y in (0..height).step_by(usize::from(TILE_SIZE)) {
        for x in (0..width).step_by(usize::from(TILE_SIZE)) {
            let tile = tile_at(width, height, x, y);
            if keep(tile) {
                tiles.push(tile);
            }
        }
    }
    tiles
}

#[must_use]
pub fn coalesce(tiles: &[Rect]) -> Vec<Rect> {
    let mut runs: Vec<Rect> = Vec::new();
    for &tile in tiles {
        match runs.last_mut() {
            Some(run) if continues(*run, tile) => run.width += tile.width,
            _ => runs.push(tile),
        }
    }
    runs
}

fn continues(run: Rect, tile: Rect) -> bool {
    run.y == tile.y
        && run.height == tile.height
        && u32::from(run.x) + u32::from(run.width) == u32::from(tile.x)
        && u32::from(run.width) + u32::from(tile.width) <= u32::from(MAX_RUN_WIDTH)
}

fn tile_at(width: u16, height: u16, x: u16, y: u16) -> Rect {
    Rect {
        x,
        y,
        width: TILE_SIZE.min(width - x),
        height: TILE_SIZE.min(height - y),
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

    fn all_changed(width: u16, height: u16) -> Bitmap {
        Bitmap::new(
            width,
            height,
            vec![1; usize::from(width) * usize::from(height) * 4],
        )
        .unwrap()
    }

    fn with_pixel(width: u16, height: u16, x: u16, y: u16) -> Bitmap {
        let mut data = vec![0; usize::from(width) * usize::from(height) * 4];
        let at = (usize::from(y) * usize::from(width) + usize::from(x)) * 4;
        data[at] = 255;
        Bitmap::new(width, height, data).unwrap()
    }

    fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
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
        assert_eq!(tiles, [rect(64, 0, 64, 64)]);
    }

    #[test]
    fn edge_tiles_are_clipped_to_the_screen() {
        let tiles = changed_tiles(&blank(200, 130), &with_pixel(200, 130, 199, 129));
        assert_eq!(tiles, [rect(192, 128, 8, 2)]);
    }

    #[test]
    fn joins_neighbours_on_the_same_row() {
        let changed = changed_tiles(&blank(200, 130), &all_changed(200, 130));
        assert_eq!(changed.len(), 4 * 3);
        assert_eq!(
            coalesce(&changed),
            [
                rect(0, 0, 200, 64),
                rect(0, 64, 200, 64),
                rect(0, 128, 200, 2)
            ]
        );
    }

    #[test]
    fn keeps_separated_tiles_apart() {
        let tiles = [rect(0, 0, 64, 64), rect(128, 0, 64, 64)];
        assert_eq!(coalesce(&tiles), tiles);
    }

    #[test]
    fn caps_the_width_of_a_run() {
        let tiles: Vec<Rect> = (0..40).map(|i| rect(i * 64, 0, 64, 64)).collect();
        let runs = coalesce(&tiles);
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|run| run.width <= MAX_RUN_WIDTH));
        assert_eq!(
            runs.iter().map(|run| u32::from(run.width)).sum::<u32>(),
            40 * 64
        );
    }

    #[test]
    fn a_resize_marks_every_tile() {
        let tiles = changed_tiles(&blank(100, 100), &blank(120, 100));
        assert_eq!(tiles.len(), 2 * 2);
        assert_eq!(coalesce(&tiles).len(), 2);
    }

    #[test]
    fn a_small_rectangle_touches_one_tile() {
        assert_eq!(
            tiles_touching(&[rect(70, 5, 3, 3)], 200, 130),
            [rect(64, 0, 64, 64)]
        );
    }

    #[test]
    fn a_rectangle_across_a_boundary_touches_both_tiles() {
        assert_eq!(
            tiles_touching(&[rect(60, 60, 8, 8)], 200, 130),
            [
                rect(0, 0, 64, 64),
                rect(64, 0, 64, 64),
                rect(0, 64, 64, 64),
                rect(64, 64, 64, 64)
            ]
        );
    }

    #[test]
    fn a_rectangle_ending_on_a_boundary_does_not_spill_over() {
        assert_eq!(
            tiles_touching(&[rect(0, 0, 64, 64)], 200, 130),
            [rect(0, 0, 64, 64)]
        );
    }

    #[test]
    fn rectangles_are_clipped_to_the_screen_and_empty_ones_ignored() {
        assert_eq!(
            tiles_touching(&[rect(190, 120, 500, 500)], 200, 130),
            [
                rect(128, 64, 64, 64),
                rect(192, 64, 8, 64),
                rect(128, 128, 64, 2),
                rect(192, 128, 8, 2)
            ]
        );
        assert_eq!(
            tiles_touching(&[rect(0, 0, 0, 10), rect(500, 0, 5, 5)], 200, 130),
            Vec::<Rect>::new()
        );
    }

    #[test]
    fn duplicate_rectangles_do_not_duplicate_tiles() {
        let same = rect(10, 10, 5, 5);
        assert_eq!(tiles_touching(&[same, same, same], 200, 130).len(), 1);
    }

    #[test]
    fn the_whole_screen_touches_every_tile() {
        assert_eq!(
            tiles_touching(&[rect(0, 0, 200, 130)], 200, 130).len(),
            4 * 3
        );
    }
}
