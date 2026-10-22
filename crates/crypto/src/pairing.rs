use blake2::Blake2sMac256;
use blake2::digest::{KeyInit, Mac};
use zeroize::Zeroize;

use crate::base32;
use crate::error::CryptoError;
use crate::random;

pub const CODE_BYTES: usize = 20;
pub const PSK_BYTES: usize = 32;
pub const VALIDITY_SECONDS: u64 = 600;
pub const MAX_FAILURES: u8 = 5;

const PSK_LABEL: &[u8] = b"wirehaus-pairing-psk-v1";

#[derive(Clone, PartialEq, Eq)]
pub struct PairingCode([u8; CODE_BYTES]);

impl std::fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairingCode(..)")
    }
}

impl Drop for PairingCode {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl PairingCode {
    pub fn generate() -> Result<Self, CryptoError> {
        let mut bytes = [0_u8; CODE_BYTES];
        random::fill(&mut bytes)?;
        Ok(Self(bytes))
    }

    pub fn from_text(text: &str) -> Result<Self, CryptoError> {
        let bytes = base32::decode(text)?;
        let array: [u8; CODE_BYTES] =
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| CryptoError::InvalidPairingCode {
                    expected: CODE_BYTES,
                    actual: bytes.len(),
                })?;
        Ok(Self(array))
    }

    #[must_use]
    pub fn to_text(&self) -> String {
        let text = base32::encode(&self.0);
        let groups: Vec<&str> = text
            .as_bytes()
            .chunks(4)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
            .collect();
        groups.join("-")
    }

    #[must_use]
    pub fn psk(&self) -> [u8; PSK_BYTES] {
        let mut mac = <Blake2sMac256 as KeyInit>::new_from_slice(&self.0)
            .expect("a 20-byte key is valid for BLAKE2s");
        mac.update(PSK_LABEL);
        mac.finalize().into_bytes().into()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PairingGrant {
    psk: [u8; PSK_BYTES],
    expires_at: u64,
    failures: u8,
}

impl std::fmt::Debug for PairingGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingGrant")
            .field("expires_at", &self.expires_at)
            .field("failures", &self.failures)
            .finish_non_exhaustive()
    }
}

impl Drop for PairingGrant {
    fn drop(&mut self) {
        self.psk.zeroize();
    }
}

impl PairingGrant {
    #[must_use]
    pub fn new(code: &PairingCode, now: u64) -> Self {
        Self {
            psk: code.psk(),
            expires_at: now.saturating_add(VALIDITY_SECONDS),
            failures: 0,
        }
    }

    #[must_use]
    pub fn from_parts(psk: [u8; PSK_BYTES], expires_at: u64, failures: u8) -> Self {
        Self {
            psk,
            expires_at,
            failures,
        }
    }

    #[must_use]
    pub fn usable(&self, now: u64) -> bool {
        now < self.expires_at && self.failures < MAX_FAILURES
    }

    #[must_use]
    pub fn psk(&self) -> [u8; PSK_BYTES] {
        self.psk
    }

    #[must_use]
    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    #[must_use]
    pub fn failures(&self) -> u8 {
        self.failures
    }

    pub fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_form_is_eight_groups_of_four_and_round_trips() {
        let code = PairingCode::generate().unwrap();
        let text = code.to_text();
        assert_eq!(text.len(), 8 * 4 + 7);
        assert!(text.split('-').all(|group| group.len() == 4));
        assert_eq!(PairingCode::from_text(&text).unwrap(), code);
        assert_eq!(PairingCode::from_text(&text.to_lowercase()).unwrap(), code);
        assert_eq!(
            PairingCode::from_text(&text.replace('-', " ")).unwrap(),
            code
        );
    }

    #[test]
    fn rejects_codes_of_the_wrong_length_or_alphabet() {
        assert!(matches!(
            PairingCode::from_text("ABCD-EFGH"),
            Err(CryptoError::InvalidPairingCode { .. })
        ));
        let too_long = "A".repeat(40);
        assert!(matches!(
            PairingCode::from_text(&too_long),
            Err(CryptoError::InvalidPairingCode { .. })
        ));
        assert_eq!(
            PairingCode::from_text(&"1".repeat(32)),
            Err(CryptoError::InvalidBase32)
        );
    }

    #[test]
    fn the_shared_key_is_deterministic_and_depends_on_the_code() {
        let a = PairingCode::from_text(&"A".repeat(32)).unwrap();
        let b = PairingCode::from_text(&"B".repeat(32)).unwrap();
        assert_eq!(a.psk(), a.psk());
        assert_ne!(a.psk(), b.psk());
        assert_ne!(a.psk(), [0; PSK_BYTES]);
    }

    #[test]
    fn codes_are_random_and_hidden_from_debug() {
        let a = PairingCode::generate().unwrap();
        let b = PairingCode::generate().unwrap();
        assert_ne!(a, b);
        assert_eq!(format!("{a:?}"), "PairingCode(..)");
    }

    #[test]
    fn a_grant_expires_after_ten_minutes() {
        let grant = PairingGrant::new(&PairingCode::generate().unwrap(), 1000);
        assert!(grant.usable(1000));
        assert!(grant.usable(1000 + VALIDITY_SECONDS - 1));
        assert!(!grant.usable(1000 + VALIDITY_SECONDS));
    }

    #[test]
    fn a_grant_is_burned_after_too_many_failures() {
        let mut grant = PairingGrant::new(&PairingCode::generate().unwrap(), 0);
        for _ in 0..MAX_FAILURES - 1 {
            grant.record_failure();
            assert!(grant.usable(1));
        }
        grant.record_failure();
        assert!(!grant.usable(1));
    }

    #[test]
    fn a_grant_does_not_expose_its_key_through_debug() {
        let grant = PairingGrant::new(&PairingCode::generate().unwrap(), 0);
        assert!(!format!("{grant:?}").contains("psk"));
    }
}
