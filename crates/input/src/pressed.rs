use wirehaus_protocol::Button;

#[derive(Debug, Default)]
pub struct Pressed {
    keys: Vec<u16>,
    buttons: Vec<Button>,
}

impl Pressed {
    pub fn key(&mut self, code: u16, pressed: bool) {
        self.keys.retain(|&held| held != code);
        if pressed {
            self.keys.push(code);
        }
    }

    pub fn button(&mut self, button: Button, pressed: bool) {
        self.buttons.retain(|&held| held != button);
        if pressed {
            self.buttons.push(button);
        }
    }

    pub fn take(&mut self) -> (Vec<u16>, Vec<Button>) {
        (
            std::mem::take(&mut self.keys),
            std::mem::take(&mut self.buttons),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_only_what_is_still_down() {
        let mut pressed = Pressed::default();
        pressed.key(42, true);
        pressed.key(30, true);
        pressed.key(42, false);
        pressed.button(Button::Left, true);
        pressed.button(Button::Right, true);
        pressed.button(Button::Left, false);
        assert_eq!(pressed.take(), (vec![30], vec![Button::Right]));
    }

    #[test]
    fn a_repeated_press_is_listed_once_and_take_empties_it() {
        let mut pressed = Pressed::default();
        pressed.key(30, true);
        pressed.key(30, true);
        pressed.button(Button::Middle, true);
        pressed.button(Button::Middle, true);
        assert_eq!(pressed.take(), (vec![30], vec![Button::Middle]));
        assert_eq!(pressed.take(), (vec![], vec![]));
    }
}
