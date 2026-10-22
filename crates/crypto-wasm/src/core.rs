use wirehaus_crypto::CryptoError;
use wirehaus_crypto::handshake::{ViewerHandshake, ViewerOutcome};
use wirehaus_crypto::keys::{KEY_BYTES, PublicKey, StaticKey};
use wirehaus_crypto::pairing::PairingCode;
use wirehaus_crypto::session::{Limits, Session};
use wirehaus_protocol::{HEADER_LEN, MAX_PAYLOAD_LEN};

pub const VIEWER_LIMITS: Limits = Limits {
    send: 4096,
    receive: HEADER_LEN + MAX_PAYLOAD_LEN,
};

pub const DEVICE_KEY_BYTES: usize = 2 * KEY_BYTES;

pub fn generate_device_key() -> Result<Vec<u8>, CryptoError> {
    let key = StaticKey::generate()?;
    let mut out = Vec::with_capacity(DEVICE_KEY_BYTES);
    out.extend_from_slice(key.private_bytes());
    out.extend_from_slice(key.public().as_bytes());
    Ok(out)
}

pub fn device_key(bytes: &[u8]) -> Result<StaticKey, CryptoError> {
    if bytes.len() != DEVICE_KEY_BYTES {
        return Err(CryptoError::InvalidKey {
            expected: DEVICE_KEY_BYTES,
            actual: bytes.len(),
        });
    }
    let (private, public) = bytes.split_at(KEY_BYTES);
    StaticKey::from_parts(private, public)
}

pub fn reconnect(
    device: &[u8],
    agent_public: &[u8],
) -> Result<(ViewerHandshake, Vec<u8>), CryptoError> {
    ViewerHandshake::reconnect(
        &device_key(device)?,
        &PublicKey::from_bytes(agent_public)?,
        VIEWER_LIMITS,
    )
}

pub fn pairing(
    device: &[u8],
    code: &str,
    name: &str,
) -> Result<(ViewerHandshake, Vec<u8>), CryptoError> {
    ViewerHandshake::pairing(
        &device_key(device)?,
        &PairingCode::from_text(code)?,
        name,
        VIEWER_LIMITS,
    )
}

pub struct Completed {
    pub session: Session,
    pub last_message: Option<Vec<u8>>,
}

pub fn complete(handshake: ViewerHandshake, reply: &[u8]) -> Result<Completed, CryptoError> {
    let ViewerOutcome {
        session,
        last_message,
    } = handshake.complete(reply)?;
    Ok(Completed {
        session,
        last_message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirehaus_crypto::handshake::{Accepted, Replied};

    #[test]
    fn a_generated_device_key_has_the_expected_layout() {
        let bytes = generate_device_key().unwrap();
        assert_eq!(bytes.len(), DEVICE_KEY_BYTES);
        let key = device_key(&bytes).unwrap();
        assert_eq!(key.public().as_bytes()[..], bytes[KEY_BYTES..]);
        assert_ne!(generate_device_key().unwrap(), bytes);
    }

    #[test]
    fn rejects_device_keys_of_the_wrong_size() {
        assert!(device_key(&[0; 63]).is_err());
        assert!(device_key(&[0; 65]).is_err());
        assert!(reconnect(&[0; 10], &[0; 32]).is_err());
        assert!(reconnect(&generate_device_key().unwrap(), &[0; 5]).is_err());
    }

    #[test]
    fn rejects_a_malformed_pairing_code_or_name() {
        let key = generate_device_key().unwrap();
        assert!(pairing(&key, "not a code", "Aula").is_err());
        let good = PairingCode::generate().unwrap().to_text();
        assert!(pairing(&key, &good, "").is_err());
        assert!(pairing(&key, &good, "Aula 3").is_ok());
    }

    #[test]
    fn a_reconnection_works_against_the_agent_side() {
        let agent = StaticKey::generate().unwrap();
        let device = generate_device_key().unwrap();
        let (handshake, first) = reconnect(&device, agent.public().as_bytes()).unwrap();
        let limits = Limits {
            send: 1 << 20,
            receive: 4096,
        };
        let accepted = Accepted::begin(&first, &agent, None, limits).unwrap();
        let Replied::Established {
            message,
            mut established,
        } = accepted.reply().unwrap()
        else {
            panic!("reconnection finishes in two messages");
        };
        let mut done = complete(handshake, &message).unwrap();
        assert!(done.last_message.is_none());
        let records = done.session.seal(b"hola").unwrap();
        assert_eq!(
            established.session.open(&records[0]).unwrap().unwrap(),
            b"hola"
        );
    }

    #[test]
    fn a_pairing_works_against_the_agent_side() {
        let agent = StaticKey::generate().unwrap();
        let device = generate_device_key().unwrap();
        let code = PairingCode::generate().unwrap();
        let (handshake, first) = pairing(&device, &code.to_text(), "Aula 3").unwrap();
        let limits = Limits {
            send: 1 << 20,
            receive: 4096,
        };
        let accepted = Accepted::begin(&first, &agent, Some(&code.psk()), limits).unwrap();
        let Replied::AwaitingFinal { message, pending } = accepted.reply().unwrap() else {
            panic!("pairing needs a third message");
        };
        let done = complete(handshake, &message).unwrap();
        let established = pending.finish(&done.last_message.unwrap()).unwrap();
        assert_eq!(established.device_name.as_deref(), Some("Aula 3"));
        assert_eq!(done.session.remote(), agent.public());
    }

    #[test]
    fn a_tampered_reply_is_an_error_not_a_panic() {
        let agent = StaticKey::generate().unwrap();
        let device = generate_device_key().unwrap();
        let (handshake, _first) = reconnect(&device, agent.public().as_bytes()).unwrap();
        assert!(complete(handshake, &[1, 2, 3]).is_err());
    }
}
