use zeroize::Zeroize;

use crate::base32;
use crate::error::CryptoError;

pub const KEY_BYTES: usize = 32;

const KEY_PARAMS: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey([u8; KEY_BYTES]);

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PublicKey({})", self.to_text())
    }
}

impl PublicKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let array: [u8; KEY_BYTES] = bytes.try_into().map_err(|_| CryptoError::InvalidKey {
            expected: KEY_BYTES,
            actual: bytes.len(),
        })?;
        Ok(Self(array))
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    #[must_use]
    pub fn to_text(&self) -> String {
        base32::encode(&self.0)
    }

    pub fn from_text(text: &str) -> Result<Self, CryptoError> {
        Self::from_bytes(&base32::decode(text)?)
    }
}

#[derive(Clone)]
pub struct StaticKey {
    private: [u8; KEY_BYTES],
    public: PublicKey,
}

impl std::fmt::Debug for StaticKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StaticKey({})", self.public.to_text())
    }
}

impl Drop for StaticKey {
    fn drop(&mut self) {
        self.private.zeroize();
    }
}

impl StaticKey {
    pub fn generate() -> Result<Self, CryptoError> {
        let params = KEY_PARAMS.parse()?;
        let mut pair = snow::Builder::new(params).generate_keypair()?;
        let key = array(&pair.private).and_then(|private| {
            Ok(Self {
                private,
                public: PublicKey::from_bytes(&pair.public)?,
            })
        });
        pair.private.zeroize();
        key
    }

    pub fn from_parts(private: &[u8], public: &[u8]) -> Result<Self, CryptoError> {
        Ok(Self {
            private: array(private)?,
            public: PublicKey::from_bytes(public)?,
        })
    }

    #[must_use]
    pub fn private_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.private
    }

    #[must_use]
    pub fn public(&self) -> PublicKey {
        self.public
    }
}

fn array(bytes: &[u8]) -> Result<[u8; KEY_BYTES], CryptoError> {
    bytes.try_into().map_err(|_| CryptoError::InvalidKey {
        expected: KEY_BYTES,
        actual: bytes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_are_distinct_and_have_the_right_size() {
        let a = StaticKey::generate().unwrap();
        let b = StaticKey::generate().unwrap();
        assert_ne!(a.public(), b.public());
        assert_ne!(a.private_bytes(), b.private_bytes());
        assert_eq!(a.public().as_bytes().len(), KEY_BYTES);
    }

    #[test]
    fn a_public_key_round_trips_through_text() {
        let key = StaticKey::generate().unwrap().public();
        assert_eq!(PublicKey::from_text(&key.to_text()).unwrap(), key);
    }

    #[test]
    fn rejects_keys_of_the_wrong_length() {
        assert_eq!(
            PublicKey::from_bytes(&[0; 31]),
            Err(CryptoError::InvalidKey {
                expected: 32,
                actual: 31
            })
        );
        assert!(StaticKey::from_parts(&[0; 33], &[0; 32]).is_err());
        assert!(PublicKey::from_text("MZXW6").is_err());
    }

    #[test]
    fn a_key_rebuilt_from_its_parts_is_the_same_key() {
        let key = StaticKey::generate().unwrap();
        let rebuilt = StaticKey::from_parts(key.private_bytes(), key.public().as_bytes()).unwrap();
        assert_eq!(rebuilt.public(), key.public());
    }

    #[test]
    fn debug_never_prints_the_private_key() {
        let key = StaticKey::generate().unwrap();
        let text = format!("{key:?}");
        assert!(text.starts_with("StaticKey("));
        assert!(!text.contains(&base32::encode(key.private_bytes())));
    }
}
