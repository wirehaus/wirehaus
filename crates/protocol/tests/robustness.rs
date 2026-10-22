use wirehaus_protocol::{
    AuthFailure, Button, Channel, ClipboardMessage, Codec, ControlMessage, DesktopMessage, Entry,
    EntryKind, FilesMessage, Frame, InputEvent, TerminalMessage, ViewerMessage,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, limit: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(limit).unwrap()).unwrap()
    }

    fn bytes(&mut self, max: usize) -> Vec<u8> {
        let length = self.below(max + 1);
        (0..length)
            .map(|_| u8::try_from(self.next() & 0xFF).unwrap())
            .collect()
    }

    fn mutate(&mut self, valid: &[u8]) -> Vec<u8> {
        let mut bytes = valid.to_vec();
        match self.below(4) {
            0 if !bytes.is_empty() => {
                let at = self.below(bytes.len());
                bytes[at] ^= 1 << self.below(8);
            }
            1 if !bytes.is_empty() => bytes.truncate(self.below(bytes.len())),
            2 => bytes.extend(self.bytes(8)),
            _ if !bytes.is_empty() => {
                let at = self.below(bytes.len());
                bytes[at] = u8::try_from(self.next() & 0xFF).unwrap();
            }
            _ => {}
        }
        bytes
    }
}

const ROUNDS: usize = 200_000;

fn file_samples() -> Vec<Vec<u8>> {
    vec![
        FilesMessage::Upload {
            root: 1,
            path: "docs/a.txt".to_owned(),
            size: 40,
        }
        .encode(),
        FilesMessage::Listing {
            entries: vec![Entry {
                name: "a".to_owned(),
                kind: EntryKind::File,
                size: 3,
                modified: 9,
            }],
            truncated: false,
        }
        .encode(),
        ClipboardMessage::Write("copiado".to_owned()).encode(),
        ClipboardMessage::Failed(wirehaus_protocol::ClipboardFailure::TooLarge).encode(),
        FilesMessage::Roots(vec!["home".to_owned()]).encode(),
        FilesMessage::Data(vec![1, 2, 3]).encode(),
    ]
}

fn valid_samples() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for channel in Channel::ALL {
        out.push(
            Frame::new(channel, 7, vec![1, 2, 3])
                .unwrap()
                .encode()
                .unwrap(),
        );
    }
    out.push(
        DesktopMessage::Tile {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
            codec: Codec::Jpeg,
            data: vec![9; 20],
        }
        .encode(),
    );
    out.push(
        DesktopMessage::Resize {
            width: 800,
            height: 600,
        }
        .encode(),
    );
    out.push(DesktopMessage::FrameEnd { sequence: 5 }.encode());
    out.push(
        DesktopMessage::Video {
            width: 64,
            height: 32,
            keyframe: true,
            data: vec![0, 0, 0, 1, 0x67, 1, 2],
        }
        .encode(),
    );
    out.push(ViewerMessage::Capabilities { video: true }.encode());
    out.push(
        TerminalMessage::Open {
            columns: 80,
            rows: 24,
        }
        .encode(),
    );
    out.push(TerminalMessage::Input(b"ls\n".to_vec()).encode());
    out.push(TerminalMessage::Exited { code: 3 }.encode());
    out.extend(file_samples());
    out.push(InputEvent::PointerMove { x: 3, y: 4 }.encode());
    out.push(
        InputEvent::Button {
            button: Button::Right,
            pressed: true,
        }
        .encode(),
    );
    out.push(
        InputEvent::Wheel {
            horizontal: -2,
            vertical: 9,
        }
        .encode(),
    );
    out.push(
        InputEvent::Key {
            code: 30,
            pressed: false,
        }
        .encode(),
    );
    out.push(ViewerMessage::FrameAck { sequence: 77 }.encode());
    out.push(ControlMessage::AuthRequired.encode());
    out.push(ControlMessage::Resume { ticket: [5; 32] }.encode());
    out.push(
        ControlMessage::SessionEnded {
            reason: wirehaus_protocol::EndReason::Idle,
        }
        .encode(),
    );
    out.push(ControlMessage::Totp { code: *b"012345" }.encode());
    out.push(
        ControlMessage::AuthFailed {
            reason: AuthFailure::Locked,
            retry_after: 30,
        }
        .encode(),
    );
    out
}

fn inputs(rng: &mut Rng) -> Vec<Vec<u8>> {
    let samples = valid_samples();
    let mut inputs = vec![rng.bytes(48)];
    for sample in &samples {
        inputs.push(rng.mutate(sample));
    }
    inputs
}

#[test]
fn no_decoder_panics_on_random_or_mutated_input() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..ROUNDS / 20 {
        for bytes in inputs(&mut rng) {
            let _ = Frame::decode(&bytes);
            let _ = DesktopMessage::decode(&bytes);
            let _ = InputEvent::decode(&bytes);
            let _ = ViewerMessage::decode(&bytes);
            let _ = ControlMessage::decode(&bytes);
            let _ = TerminalMessage::decode(&bytes);
            let _ = FilesMessage::decode(&bytes);
            let _ = ClipboardMessage::decode(&bytes);
        }
    }
}

#[test]
fn anything_that_decodes_encodes_back_to_the_same_bytes() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut accepted = [0_usize; 5];
    for _ in 0..ROUNDS / 20 {
        for bytes in inputs(&mut rng) {
            if let Ok(frame) = Frame::decode(&bytes) {
                assert_eq!(frame.encode().unwrap(), bytes);
                accepted[0] += 1;
            }
            if let Ok(message) = DesktopMessage::decode(&bytes) {
                assert_eq!(message.encode(), bytes);
                accepted[1] += 1;
            }
            if let Ok(event) = InputEvent::decode(&bytes) {
                assert_eq!(event.encode(), bytes);
                accepted[2] += 1;
            }
            if let Ok(message) = ViewerMessage::decode(&bytes) {
                assert_eq!(message.encode(), bytes);
                accepted[3] += 1;
            }
            if let Ok(message) = TerminalMessage::decode(&bytes) {
                assert_eq!(message.encode(), bytes);
            }
            if let Ok(message) = FilesMessage::decode(&bytes) {
                assert_eq!(message.encode(), bytes);
            }
            if let Ok(message) = ClipboardMessage::decode(&bytes) {
                assert_eq!(message.encode(), bytes);
            }
            if let Ok(message) = ControlMessage::decode(&bytes) {
                assert_eq!(message.encode(), bytes);
                accepted[4] += 1;
            }
        }
    }
    assert!(accepted.iter().all(|&count| count > 100), "{accepted:?}");
}

#[test]
fn a_frame_never_accepts_a_declared_length_that_does_not_match() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    for _ in 0..ROUNDS {
        let mut bytes = Frame::new(
            Channel::Desktop,
            u32::try_from(rng.next() & 0xFFFF_FFFF).unwrap(),
            rng.bytes(32),
        )
        .unwrap()
        .encode()
        .unwrap();
        let actual = bytes.len() - 9;
        let wrong = u32::try_from((actual + 1 + rng.below(1000)) % 5000).unwrap();
        if usize::try_from(wrong).unwrap() == actual {
            continue;
        }
        bytes[5..9].copy_from_slice(&wrong.to_be_bytes());
        assert!(Frame::decode(&bytes).is_err());
    }
}
