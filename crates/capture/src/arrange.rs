use crate::bitmap::Bitmap;
use crate::error::CaptureError;
use crate::{Monitor, Monitors};

pub type OutputRect = (i32, i32, i32, i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Output(usize),
    All,
}

fn size(rect: OutputRect) -> (i32, i32) {
    (rect.2 - rect.0, rect.3 - rect.1)
}

pub fn primary_output(rects: &[OutputRect], wanted: (u16, u16)) -> Option<usize> {
    let wanted = (i32::from(wanted.0), i32::from(wanted.1));
    let fits = |rect: &OutputRect| {
        let (width, height) = size(*rect);
        (width, height) == wanted || (height, width) == wanted
    };
    rects
        .iter()
        .position(|rect| rect.0 == 0 && rect.1 == 0 && fits(rect))
        .or_else(|| rects.iter().position(fits))
}

pub fn order(rects: &[OutputRect], primary: usize) -> Vec<usize> {
    let mut others: Vec<usize> = (0..rects.len()).filter(|&index| index != primary).collect();
    others.sort_by_key(|&index| (rects[index].0, rects[index].1, index));
    let mut order = Vec::with_capacity(rects.len());
    if primary < rects.len() {
        order.push(primary);
    }
    order.extend(others);
    order
}

pub fn bounds(rects: &[OutputRect]) -> Option<OutputRect> {
    let first = *rects.first()?;
    Some(rects.iter().fold(first, |all, rect| {
        (
            all.0.min(rect.0),
            all.1.min(rect.1),
            all.2.max(rect.2),
            all.3.max(rect.3),
        )
    }))
}

fn clamp(value: i32) -> u16 {
    u16::try_from(value.max(0)).unwrap_or(u16::MAX)
}

fn whole_desktop(rects: &[OutputRect]) -> Option<OutputRect> {
    let all = bounds(rects)?;
    let (width, height) = size(all);
    (u16::try_from(width).is_ok() && u16::try_from(height).is_ok()).then_some(all)
}

pub fn monitors(rects: &[OutputRect], primary: usize, view: View) -> Monitors {
    let order = order(rects, primary);
    if order.len() < 2 {
        return Monitors::default();
    }
    let mut items: Vec<Monitor> = order
        .iter()
        .map(|&index| {
            let rect = rects[index];
            let (width, height) = size(rect);
            Monitor {
                x: rect.0,
                y: rect.1,
                width: clamp(width),
                height: clamp(height),
                primary: index == primary,
                all: false,
            }
        })
        .collect();
    let all = whole_desktop(rects);
    let active = match view {
        View::Output(output) => order.iter().position(|&index| index == output).unwrap_or(0),
        View::All => items.len(),
    };
    if let Some(all) = all {
        let (width, height) = size(all);
        items.push(Monitor {
            x: all.0,
            y: all.1,
            width: clamp(width),
            height: clamp(height),
            primary: false,
            all: true,
        });
    }
    Monitors {
        items,
        active: u8::try_from(active).unwrap_or(0),
    }
}

pub fn view_for(rects: &[OutputRect], primary: usize, index: u8) -> Option<View> {
    let order = order(rects, primary);
    if order.len() < 2 {
        return None;
    }
    let index = usize::from(index);
    match index.cmp(&order.len()) {
        std::cmp::Ordering::Less => Some(View::Output(order[index])),
        std::cmp::Ordering::Equal => whole_desktop(rects).map(|_| View::All),
        std::cmp::Ordering::Greater => None,
    }
}

pub fn compose(
    area: OutputRect,
    parts: &[(OutputRect, Option<&Bitmap>)],
) -> Result<Bitmap, CaptureError> {
    let (width, height) = size(area);
    let invalid = || CaptureError::InvalidSize {
        width: clamp(width),
        height: clamp(height),
    };
    let columns = u16::try_from(width).map_err(|_| invalid())?;
    let rows = u16::try_from(height).map_err(|_| invalid())?;
    let mut pixels = vec![0u8; usize::from(columns) * usize::from(rows) * 4];
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    let mut canvas = Bitmap::new(columns, rows, pixels)?;
    for (rect, bitmap) in parts {
        let Some(bitmap) = bitmap else { continue };
        let (Ok(x), Ok(y)) = (
            u16::try_from(rect.0 - area.0),
            u16::try_from(rect.1 - area.1),
        ) else {
            continue;
        };
        let _ = canvas.paste(x, y, bitmap);
    }
    Ok(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: OutputRect = (-1920, 0, 0, 1080);
    const MAIN: OutputRect = (0, 0, 1920, 1080);
    const RIGHT: OutputRect = (1920, 100, 3840, 1180);

    fn solid(width: u16, height: u16, value: u8) -> Bitmap {
        let mut pixels = vec![value; usize::from(width) * usize::from(height) * 4];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        Bitmap::new(width, height, pixels).unwrap()
    }

    #[test]
    fn the_primary_is_the_output_at_the_origin_even_with_identical_sizes() {
        assert_eq!(primary_output(&[LEFT, MAIN], (1920, 1080)), Some(1));
        assert_eq!(primary_output(&[MAIN, LEFT], (1920, 1080)), Some(0));
    }

    #[test]
    fn a_rotated_primary_still_matches() {
        assert_eq!(primary_output(&[(0, 0, 1080, 1920)], (1920, 1080)), Some(0));
        assert_eq!(primary_output(&[MAIN], (2560, 1440)), None);
    }

    #[test]
    fn the_primary_goes_first_and_the_rest_follow_the_desktop_layout() {
        assert_eq!(order(&[RIGHT, MAIN, LEFT], 1), vec![1, 2, 0]);
    }

    #[test]
    fn a_single_output_offers_no_choice() {
        assert_eq!(monitors(&[MAIN], 0, View::Output(0)), Monitors::default());
        assert_eq!(view_for(&[MAIN], 0, 0), None);
    }

    #[test]
    fn the_list_ends_with_the_whole_desktop() {
        let list = monitors(&[LEFT, MAIN], 1, View::Output(1));
        assert_eq!(list.items.len(), 3);
        assert_eq!(list.active, 0);
        assert!(list.items[0].primary);
        assert_eq!((list.items[1].x, list.items[1].y), (-1920, 0));
        let all = list.items[2];
        assert!(all.all);
        assert_eq!(
            (all.x, all.y, all.width, all.height),
            (-1920, 0, 3840, 1080)
        );
    }

    #[test]
    fn the_active_entry_follows_the_view() {
        assert_eq!(monitors(&[LEFT, MAIN], 1, View::Output(0)).active, 1);
        assert_eq!(monitors(&[LEFT, MAIN], 1, View::All).active, 2);
    }

    #[test]
    fn an_index_maps_to_an_output_the_whole_desktop_or_nothing() {
        assert_eq!(view_for(&[LEFT, MAIN], 1, 0), Some(View::Output(1)));
        assert_eq!(view_for(&[LEFT, MAIN], 1, 1), Some(View::Output(0)));
        assert_eq!(view_for(&[LEFT, MAIN], 1, 2), Some(View::All));
        assert_eq!(view_for(&[LEFT, MAIN], 1, 3), None);
    }

    #[test]
    fn bounds_cover_every_output() {
        assert_eq!(bounds(&[LEFT, MAIN, RIGHT]), Some((-1920, 0, 3840, 1180)));
        assert_eq!(bounds(&[]), None);
    }

    #[test]
    fn composing_places_each_output_at_its_offset_and_leaves_gaps_black() {
        let area = bounds(&[LEFT, RIGHT]).unwrap();
        let left = solid(1920, 1080, 10);
        let right = solid(1920, 1080, 200);
        let canvas = compose(area, &[(LEFT, Some(&left)), (RIGHT, Some(&right))]).unwrap();
        assert_eq!((canvas.width(), canvas.height()), (5760, 1180));
        let pixel = |x: u16, y: u16| canvas.row(y)[usize::from(x) * 4];
        assert_eq!(pixel(0, 0), 10);
        assert_eq!(pixel(3840, 100), 200);
        assert_eq!(pixel(3840, 0), 0);
        assert_eq!(canvas.row(0)[3], 255);
    }

    #[test]
    fn an_output_without_a_frame_yet_stays_black() {
        let canvas = compose(MAIN, &[(MAIN, None)]).unwrap();
        assert_eq!(canvas.row(0)[0], 0);
    }

    #[test]
    fn a_desktop_wider_than_the_protocol_allows_is_not_offered() {
        let rects = [(0, 0, 40_000, 100), (40_000, 0, 80_000, 100)];
        assert_eq!(monitors(&rects, 0, View::Output(0)).items.len(), 2);
        assert_eq!(view_for(&rects, 0, 2), None);
    }

    #[test]
    fn a_desktop_wider_than_the_protocol_allows_is_refused() {
        assert!(compose((0, 0, 70_000, 100), &[]).is_err());
    }
}
