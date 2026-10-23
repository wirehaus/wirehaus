const EXTENDED: u16 = 0xE000;

const EXTENDED_KEYS: [(u16, u16); 18] = [
    (96, 0x1C),
    (97, 0x1D),
    (98, 0x35),
    (99, 0x37),
    (100, 0x38),
    (102, 0x47),
    (103, 0x48),
    (104, 0x49),
    (105, 0x4B),
    (106, 0x4D),
    (107, 0x4F),
    (108, 0x50),
    (109, 0x51),
    (110, 0x52),
    (111, 0x53),
    (125, 0x5B),
    (126, 0x5C),
    (127, 0x5D),
];

const LAST_PLAIN_KEY: u16 = 88;

pub fn scancode(evdev: u16) -> Option<u16> {
    if (1..=LAST_PLAIN_KEY).contains(&evdev) {
        return Some(evdev);
    }
    EXTENDED_KEYS
        .iter()
        .find(|(code, _)| *code == evdev)
        .map(|(_, scan)| EXTENDED | scan)
}

pub fn wheel_notches(horizontal: i16, vertical: i16) -> (i32, i32) {
    let clamp = |amount: i32| amount.clamp(-MAX_NOTCHES, MAX_NOTCHES);
    (clamp(i32::from(horizontal)), clamp(-i32::from(vertical)))
}

const MAX_NOTCHES: i32 = 32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_keys_keep_their_number() {
        assert_eq!(scancode(1), Some(0x01));
        assert_eq!(scancode(30), Some(0x1E));
        assert_eq!(scancode(57), Some(0x39));
        assert_eq!(scancode(86), Some(0x56));
        assert_eq!(scancode(88), Some(0x58));
    }

    #[test]
    fn navigation_and_right_hand_modifiers_are_extended() {
        assert_eq!(scancode(103), Some(0xE048));
        assert_eq!(scancode(111), Some(0xE053));
        assert_eq!(scancode(97), Some(0xE01D));
        assert_eq!(scancode(100), Some(0xE038));
        assert_eq!(scancode(125), Some(0xE05B));
        assert_eq!(scancode(96), Some(0xE01C));
    }

    #[test]
    fn unsupported_codes_are_rejected() {
        assert_eq!(scancode(0), None);
        assert_eq!(scancode(119), None);
        assert_eq!(scancode(89), None);
        assert_eq!(scancode(300), None);
    }

    #[test]
    fn wheel_is_clamped_and_vertical_follows_the_windows_sign() {
        assert_eq!(wheel_notches(0, 3), (0, -3));
        assert_eq!(wheel_notches(-2, -1), (-2, 1));
        assert_eq!(wheel_notches(500, -500), (32, 32));
    }
}
