#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
}

impl Area {
    #[must_use]
    pub fn is_primary(self, primary: (i32, i32)) -> bool {
        self.x == 0
            && self.y == 0
            && i32::from(self.width) == primary.0
            && i32::from(self.height) == primary.1
    }

    #[must_use]
    pub fn locate(self, x: u16, y: u16) -> Option<(i32, i32)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some((self.x + i32::from(x), self.y + i32::from(y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Area = Area {
        x: -1920,
        y: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn a_point_is_offset_by_the_origin_of_the_monitor() {
        assert_eq!(LEFT.locate(0, 0), Some((-1920, 0)));
        assert_eq!(LEFT.locate(1919, 1079), Some((-1, 1079)));
    }

    #[test]
    fn points_outside_the_monitor_are_refused() {
        assert_eq!(LEFT.locate(1920, 0), None);
        assert_eq!(LEFT.locate(0, 1080), None);
    }

    #[test]
    fn only_the_origin_monitor_of_the_primary_size_counts_as_primary() {
        let primary = Area {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(primary.is_primary((1920, 1080)));
        assert!(!LEFT.is_primary((1920, 1080)));
        let both = Area {
            width: 3840,
            ..primary
        };
        assert!(!both.is_primary((1920, 1080)));
    }
}
