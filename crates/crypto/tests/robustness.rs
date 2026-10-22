use wirehaus_crypto::base32;
use wirehaus_crypto::handshake::{Accepted, Mode, Replied, ViewerHandshake};
use wirehaus_crypto::keys::{PublicKey, StaticKey};
use wirehaus_crypto::pairing::PairingCode;
use wirehaus_crypto::session::{Limits, Session};
use wirehaus_crypto::totp::TotpSecret;

const LIMITS: Limits = Limits {
    send: 1 << 20,
    receive: 1 << 20,
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

    fn text(&mut self, max: usize) -> String {
        const ALPHABET: &[u8] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567abcdefghijklmnopqrstuvwxyz0189- =\n\t\xC3\xB1";
        let length = self.below(max + 1);
        let bytes: Vec<u8> = (0..length)
            .map(|_| ALPHABET[self.below(ALPHABET.len())])
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
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

#[test]
fn base32_never_panics_and_is_canonical() {
    let mut rng = Rng(0xA5A5_5A5A_1234_4321);
    let mut decoded = 0;
    for _ in 0..100_000 {
        let text = rng.text(40);
        if let Ok(bytes) = base32::decode(&text) {
            decoded += 1;
            let normalised: String = text
                .chars()
                .filter(|c| !matches!(c, '-' | ' '))
                .map(|c| c.to_ascii_uppercase())
                .collect();
            assert_eq!(base32::encode(&bytes), normalised);
        }
    }
    assert!(decoded > 100);
}

#[test]
fn keys_codes_and_totp_inputs_never_panic() {
    let mut rng = Rng(0x0F0F_F0F0_1357_9BDF);
    let secret = TotpSecret::generate().unwrap();
    for _ in 0..50_000 {
        let text = rng.text(48);
        let _ = PairingCode::from_text(&text);
        let _ = PublicKey::from_text(&text);
        let _ = secret.verify(&text, rng.next() >> 20, None);
        let _ = StaticKey::from_parts(&rng.bytes(70), &rng.bytes(70));
        let _ = PublicKey::from_bytes(&rng.bytes(70));
    }
}

fn first_messages(agent: &StaticKey, device: &StaticKey, code: &PairingCode) -> Vec<Vec<u8>> {
    let (_, reconnect) = ViewerHandshake::reconnect(device, &agent.public(), LIMITS).unwrap();
    let (_, pairing) = ViewerHandshake::pairing(device, code, "Aula", LIMITS).unwrap();
    vec![reconnect, pairing]
}

#[test]
fn the_agent_never_panics_or_accepts_mutated_first_messages() {
    let mut rng = Rng(0x7777_AAAA_5555_CCCC);
    let agent = StaticKey::generate().unwrap();
    let device = StaticKey::generate().unwrap();
    let code = PairingCode::generate().unwrap();
    let psk = code.psk();
    let valid = first_messages(&agent, &device, &code);
    for round in 0..1500 {
        let input = if round % 2 == 0 {
            rng.bytes(200)
        } else {
            let pick = rng.below(valid.len());
            rng.mutate(&valid[pick])
        };
        let exact = valid.contains(&input);
        let result = Accepted::begin(&input, &agent, Some(&psk), LIMITS);
        if !exact {
            assert!(result.is_err(), "a mutated first message was accepted");
        }
    }
}

#[test]
fn the_viewer_never_panics_or_accepts_a_forged_reply() {
    let mut rng = Rng(0x1111_2222_3333_4444);
    let agent = StaticKey::generate().unwrap();
    let device = StaticKey::generate().unwrap();
    for round in 0..600 {
        let (handshake, first) =
            ViewerHandshake::reconnect(&device, &agent.public(), LIMITS).unwrap();
        let accepted = Accepted::begin(&first, &agent, None, LIMITS).unwrap();
        let Replied::Established { message, .. } = accepted.reply().unwrap() else {
            panic!("unexpected reply");
        };
        let forged = if round % 2 == 0 {
            rng.bytes(100)
        } else {
            let mut mutated = rng.mutate(&message);
            if mutated == message {
                mutated.push(0);
            }
            mutated
        };
        assert!(handshake.complete(&forged).is_err());
    }
}

fn established_pair() -> (Session, Session) {
    let agent = StaticKey::generate().unwrap();
    let device = StaticKey::generate().unwrap();
    let (handshake, first) = ViewerHandshake::reconnect(&device, &agent.public(), LIMITS).unwrap();
    let accepted = Accepted::begin(&first, &agent, None, LIMITS).unwrap();
    assert_eq!(accepted.mode(), Mode::Reconnect);
    let Replied::Established {
        message,
        established,
    } = accepted.reply().unwrap()
    else {
        panic!("unexpected reply");
    };
    (
        handshake.complete(&message).unwrap().session,
        established.session,
    )
}

#[test]
fn a_session_never_panics_and_never_accepts_garbage_records() {
    let mut rng = Rng(0x0BAD_C0DE_DEAD_BEEF);
    for _ in 0..100 {
        let (mut viewer, mut agent) = established_pair();
        let genuine = viewer.seal(b"genuine message").unwrap();
        for _ in 0..8 {
            let junk = if rng.below(2) == 0 {
                rng.bytes(300)
            } else {
                rng.mutate(&genuine[0])
            };
            if junk == genuine[0] {
                continue;
            }
            assert!(agent.open(&junk).is_err());
        }
        assert!(
            agent.open(&genuine[0]).is_err(),
            "a failed session kept working"
        );
    }
}
