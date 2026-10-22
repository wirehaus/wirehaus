use std::time::{Duration, Instant};

use wirehaus_protocol::Button;

const MAX_WHEEL_NOTCHES: i32 = 32;
const CLICK_INTERVAL: Duration = Duration::from_millis(500);
const CLICK_DISTANCE: f64 = 5.0;

const KEYCODES: [(u16, u16); 113] = [
    (1, 53),
    (2, 18),
    (3, 19),
    (4, 20),
    (5, 21),
    (6, 23),
    (7, 22),
    (8, 26),
    (9, 28),
    (10, 25),
    (11, 29),
    (12, 27),
    (13, 24),
    (14, 51),
    (15, 48),
    (16, 12),
    (17, 13),
    (18, 14),
    (19, 15),
    (20, 17),
    (21, 16),
    (22, 32),
    (23, 34),
    (24, 31),
    (25, 35),
    (26, 33),
    (27, 30),
    (28, 36),
    (29, 59),
    (30, 0),
    (31, 1),
    (32, 2),
    (33, 3),
    (34, 5),
    (35, 4),
    (36, 38),
    (37, 40),
    (38, 37),
    (39, 41),
    (40, 39),
    (41, 50),
    (42, 56),
    (43, 42),
    (44, 6),
    (45, 7),
    (46, 8),
    (47, 9),
    (48, 11),
    (49, 45),
    (50, 46),
    (51, 43),
    (52, 47),
    (53, 44),
    (54, 60),
    (55, 67),
    (56, 58),
    (57, 49),
    (58, 57),
    (59, 122),
    (60, 120),
    (61, 99),
    (62, 118),
    (63, 96),
    (64, 97),
    (65, 98),
    (66, 100),
    (67, 101),
    (68, 109),
    (69, 71),
    (71, 89),
    (72, 91),
    (73, 92),
    (74, 78),
    (75, 86),
    (76, 87),
    (77, 88),
    (78, 69),
    (79, 83),
    (80, 84),
    (81, 85),
    (82, 82),
    (83, 65),
    (86, 10),
    (87, 103),
    (88, 111),
    (89, 94),
    (96, 76),
    (97, 62),
    (98, 75),
    (100, 61),
    (102, 115),
    (103, 126),
    (104, 116),
    (105, 123),
    (106, 124),
    (107, 119),
    (108, 125),
    (109, 121),
    (110, 114),
    (111, 117),
    (113, 74),
    (114, 73),
    (115, 72),
    (125, 55),
    (126, 54),
    (183, 105),
    (184, 107),
    (185, 113),
    (186, 106),
    (187, 64),
    (188, 79),
    (189, 80),
    (190, 90),
];

pub fn mac_keycode(evdev: u16) -> Option<u16> {
    KEYCODES
        .iter()
        .find(|(code, _)| *code == evdev)
        .map(|&(_, mac)| mac)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Control,
    Option,
    Command,
}

pub fn modifier_of(mac_keycode: u16) -> Option<Modifier> {
    match mac_keycode {
        56 | 60 => Some(Modifier::Shift),
        59 | 62 => Some(Modifier::Control),
        58 | 61 => Some(Modifier::Option),
        54 | 55 => Some(Modifier::Command),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub struct Modifiers {
    held: Vec<u16>,
}

impl Modifiers {
    pub fn key(&mut self, mac_keycode: u16, pressed: bool) {
        if modifier_of(mac_keycode).is_none() {
            return;
        }
        self.held.retain(|&held| held != mac_keycode);
        if pressed {
            self.held.push(mac_keycode);
        }
    }

    pub fn active(&self, modifier: Modifier) -> bool {
        self.held
            .iter()
            .any(|&held| modifier_of(held) == Some(modifier))
    }
}

#[derive(Debug, Default)]
pub struct ClickTracker {
    last: Option<(Button, f64, f64, Instant)>,
    count: i64,
}

impl ClickTracker {
    pub fn press(&mut self, button: Button, x: f64, y: f64, now: Instant) -> i64 {
        let repeats = self.last.is_some_and(|(last, last_x, last_y, at)| {
            last == button
                && now.saturating_duration_since(at) <= CLICK_INTERVAL
                && (x - last_x).abs() <= CLICK_DISTANCE
                && (y - last_y).abs() <= CLICK_DISTANCE
        });
        self.count = if repeats { self.count + 1 } else { 1 };
        self.last = Some((button, x, y, now));
        self.count
    }

    pub fn current(&self) -> i64 {
        self.count.max(1)
    }
}

pub fn wheel_lines(horizontal: i16, vertical: i16) -> (i32, i32) {
    let clamp = |amount: i32| amount.clamp(-MAX_WHEEL_NOTCHES, MAX_WHEEL_NOTCHES);
    (clamp(i32::from(vertical)), clamp(-i32::from(horizontal)))
}

pub fn to_points(pixel: u16, points: f64, pixels: f64) -> f64 {
    if pixels <= 0.0 {
        return f64::from(pixel);
    }
    f64::from(pixel) * points / pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_letters_digits_and_common_keys_to_mac_virtual_keycodes() {
        assert_eq!(mac_keycode(30), Some(0));
        assert_eq!(mac_keycode(16), Some(12));
        assert_eq!(mac_keycode(44), Some(6));
        assert_eq!(mac_keycode(2), Some(18));
        assert_eq!(mac_keycode(11), Some(29));
        assert_eq!(mac_keycode(28), Some(36));
        assert_eq!(mac_keycode(57), Some(49));
        assert_eq!(mac_keycode(14), Some(51));
        assert_eq!(mac_keycode(111), Some(117));
        assert_eq!(mac_keycode(125), Some(55));
        assert_eq!(mac_keycode(103), Some(126));
    }

    #[test]
    fn never_maps_two_keys_to_the_same_mac_keycode() {
        let mut seen = std::collections::HashSet::new();
        let mut evdev = std::collections::HashSet::new();
        for (code, mac) in KEYCODES {
            assert!(evdev.insert(code), "evdev {code} listed twice");
            assert!(seen.insert(mac), "mac keycode {mac} used twice");
            assert!(mac < 128);
        }
    }

    #[test]
    fn unknown_keys_are_not_mapped() {
        assert_eq!(mac_keycode(0), None);
        assert_eq!(mac_keycode(70), None);
        assert_eq!(mac_keycode(u16::MAX), None);
    }

    #[test]
    fn every_letter_row_key_is_mapped() {
        for code in (16..=25).chain(30..=38).chain(44..=50).chain(2..=11) {
            assert!(mac_keycode(code).is_some(), "{code}");
        }
    }

    #[test]
    fn modifiers_follow_the_keys_that_are_down() {
        let mut modifiers = Modifiers::default();
        modifiers.key(55, true);
        modifiers.key(56, true);
        assert!(modifiers.active(Modifier::Command));
        assert!(modifiers.active(Modifier::Shift));
        assert!(!modifiers.active(Modifier::Control));
        modifiers.key(55, false);
        assert!(!modifiers.active(Modifier::Command));
        assert!(modifiers.active(Modifier::Shift));
    }

    #[test]
    fn a_modifier_stays_active_while_either_side_is_down() {
        let mut modifiers = Modifiers::default();
        modifiers.key(56, true);
        modifiers.key(60, true);
        modifiers.key(56, false);
        assert!(modifiers.active(Modifier::Shift));
        modifiers.key(60, false);
        assert!(!modifiers.active(Modifier::Shift));
    }

    #[test]
    fn a_repeated_press_does_not_stack_and_other_keys_are_ignored() {
        let mut modifiers = Modifiers::default();
        modifiers.key(59, true);
        modifiers.key(59, true);
        modifiers.key(0, true);
        modifiers.key(59, false);
        assert!(!modifiers.active(Modifier::Control));
    }

    #[test]
    fn quick_clicks_in_the_same_place_count_up() {
        let mut clicks = ClickTracker::default();
        let start = Instant::now();
        assert_eq!(clicks.press(Button::Left, 100.0, 100.0, start), 1);
        let second = start + Duration::from_millis(200);
        assert_eq!(clicks.press(Button::Left, 102.0, 99.0, second), 2);
        let third = second + Duration::from_millis(200);
        assert_eq!(clicks.press(Button::Left, 102.0, 99.0, third), 3);
        assert_eq!(clicks.current(), 3);
    }

    #[test]
    fn slow_distant_or_different_button_clicks_start_over() {
        let mut clicks = ClickTracker::default();
        let start = Instant::now();
        clicks.press(Button::Left, 100.0, 100.0, start);
        let slow = start + Duration::from_millis(900);
        assert_eq!(clicks.press(Button::Left, 100.0, 100.0, slow), 1);
        let far = slow + Duration::from_millis(100);
        assert_eq!(clicks.press(Button::Left, 140.0, 100.0, far), 1);
        let other = far + Duration::from_millis(100);
        assert_eq!(clicks.press(Button::Right, 140.0, 100.0, other), 1);
    }

    #[test]
    fn up_and_right_follow_the_mac_wheel_axes() {
        assert_eq!(wheel_lines(0, 3), (3, 0));
        assert_eq!(wheel_lines(0, -3), (-3, 0));
        assert_eq!(wheel_lines(4, 0), (0, -4));
        assert_eq!(wheel_lines(-4, 0), (0, 4));
    }

    #[test]
    fn the_wheel_is_clamped() {
        assert_eq!(wheel_lines(i16::MAX, i16::MAX), (32, -32));
        assert_eq!(wheel_lines(i16::MIN, i16::MIN), (-32, 32));
    }

    #[test]
    fn scales_pixels_to_points() {
        assert!((to_points(2000, 1000.0, 2000.0) - 1000.0).abs() < f64::EPSILON);
        assert!((to_points(1000, 1512.0, 3024.0) - 500.0).abs() < f64::EPSILON);
        assert!((to_points(10, 100.0, 0.0) - 10.0).abs() < f64::EPSILON);
    }
}
