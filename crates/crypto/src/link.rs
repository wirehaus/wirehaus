use blake2::Blake2s256;
use blake2::digest::Digest;
use snow::Builder;
use zeroize::Zeroize;

use crate::error::CryptoError;
use crate::keys::KEY_BYTES;

const PARAMS: &str = "Noise_N_25519_ChaChaPoly_BLAKE2s";
const MAX_SEALED: usize = 1024;

pub const ID_BYTES: usize = 16;

pub fn new_id() -> Result<[u8; ID_BYTES], CryptoError> {
    let mut id = [0_u8; ID_BYTES];
    crate::random::fill(&mut id)?;
    Ok(id)
}

pub fn generate() -> Result<([u8; KEY_BYTES], [u8; KEY_BYTES]), CryptoError> {
    let mut pair = Builder::new(PARAMS.parse()?).generate_keypair()?;
    let keys = match (
        <[u8; KEY_BYTES]>::try_from(pair.private.as_slice()),
        <[u8; KEY_BYTES]>::try_from(pair.public.as_slice()),
    ) {
        (Ok(private), Ok(public)) => Ok((private, public)),
        _ => Err(CryptoError::InvalidKey {
            expected: KEY_BYTES,
            actual: pair.public.len(),
        }),
    };
    pair.private.zeroize();
    keys
}

pub fn seal(recipient_public: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if recipient_public.len() != KEY_BYTES {
        return Err(CryptoError::InvalidKey {
            expected: KEY_BYTES,
            actual: recipient_public.len(),
        });
    }
    if plaintext.len() > MAX_SEALED {
        return Err(CryptoError::MessageTooLarge {
            len: plaintext.len(),
            max: MAX_SEALED,
        });
    }
    let mut state = Builder::new(PARAMS.parse()?)
        .remote_public_key(recipient_public)?
        .build_initiator()?;
    let mut out = vec![0_u8; MAX_SEALED + 128];
    let written = state.write_message(plaintext, &mut out)?;
    out.truncate(written);
    Ok(out)
}

pub fn open(private: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if private.len() != KEY_BYTES {
        return Err(CryptoError::InvalidKey {
            expected: KEY_BYTES,
            actual: private.len(),
        });
    }
    if sealed.len() > MAX_SEALED + 128 {
        return Err(CryptoError::MessageTooLarge {
            len: sealed.len(),
            max: MAX_SEALED + 128,
        });
    }
    let mut state = Builder::new(PARAMS.parse()?)
        .local_private_key(private)?
        .build_responder()?;
    let mut out = vec![0_u8; sealed.len()];
    let read = state.read_message(sealed, &mut out)?;
    out.truncate(read);
    Ok(out)
}

#[must_use]
pub fn verification_code(public: &[u8], id: &[u8]) -> String {
    let mut hasher = Blake2s256::new();
    hasher.update(b"wirehaus link verification");
    hasher.update(public);
    hasher.update(id);
    let digest = hasher.finalize();
    let number = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 10_000;
    format!("{number:04}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_message_opens_only_with_the_matching_private_key() {
        let (private, public) = generate().unwrap();
        let (other_private, _) = generate().unwrap();
        let sealed = seal(&public, b"ABCD-EFGH-IJKL").unwrap();
        assert_eq!(open(&private, &sealed).unwrap(), b"ABCD-EFGH-IJKL");
        assert!(open(&other_private, &sealed).is_err());
    }

    #[test]
    fn sealing_twice_gives_different_ciphertexts_and_hides_the_text() {
        let (_, public) = generate().unwrap();
        let one = seal(&public, b"same text").unwrap();
        let two = seal(&public, b"same text").unwrap();
        assert_ne!(one, two);
        assert!(!one.windows(9).any(|w| w == b"same text"));
    }

    #[test]
    fn a_tampered_or_truncated_message_is_refused() {
        let (private, public) = generate().unwrap();
        let mut sealed = seal(&public, b"secret").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert!(open(&private, &sealed).is_err());
        assert!(open(&private, &sealed[..10]).is_err());
        assert!(open(&private, &[]).is_err());
    }

    #[test]
    fn bad_key_lengths_and_oversized_texts_are_refused() {
        assert!(seal(&[0; 5], b"x").is_err());
        assert!(open(&[0; 5], b"x").is_err());
        let (_, public) = generate().unwrap();
        assert!(seal(&public, &vec![0; MAX_SEALED + 1]).is_err());
    }

    #[test]
    fn the_verification_code_has_four_digits_and_depends_on_both_inputs() {
        let a = verification_code(&[1; 32], &[2; ID_BYTES]);
        assert_eq!(a.len(), 4);
        assert!(a.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(a, verification_code(&[1; 32], &[2; ID_BYTES]));
        let different = (0..20)
            .filter(|n| verification_code(&[*n; 32], &[2; ID_BYTES]) != a)
            .count();
        assert!(different > 15);
    }
}
