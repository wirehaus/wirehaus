use std::fmt::Write;

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::base32;
use crate::error::CryptoError;
use crate::random;

pub const SECRET_BYTES: usize = 20;
pub const STEP_SECONDS: u64 = 30;
pub const DIGITS: u32 = 6;

#[derive(Clone, PartialEq, Eq)]
pub struct TotpSecret([u8; SECRET_BYTES]);

impl std::fmt::Debug for TotpSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TotpSecret(..)")
    }
}

impl Drop for TotpSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl TotpSecret {
    pub fn generate() -> Result<Self, CryptoError> {
        let mut bytes = [0_u8; SECRET_BYTES];
        random::fill(&mut bytes)?;
        Ok(Self(bytes))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let array: [u8; SECRET_BYTES] =
            bytes.try_into().map_err(|_| CryptoError::InvalidSecret {
                expected: SECRET_BYTES,
                actual: bytes.len(),
            })?;
        Ok(Self(array))
    }

    #[must_use]
    pub fn to_bytes(&self) -> [u8; SECRET_BYTES] {
        self.0
    }

    #[must_use]
    pub fn to_base32(&self) -> String {
        base32::encode(&self.0)
    }

    #[must_use]
    pub fn otpauth_uri(&self, issuer: &str, account: &str) -> String {
        format!(
            "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECONDS}",
            percent_encode(issuer),
            percent_encode(account),
            self.to_base32(),
            percent_encode(issuer),
        )
    }

    #[must_use]
    pub fn code_at_step(&self, step: u64) -> String {
        format!(
            "{:0width$}",
            hotp(&self.0, step, DIGITS),
            width = DIGITS as usize
        )
    }

    #[must_use]
    pub fn step_at(unix_time: u64) -> u64 {
        unix_time / STEP_SECONDS
    }

    #[must_use]
    pub fn verify(&self, code: &str, unix_time: u64, last_accepted: Option<u64>) -> Option<u64> {
        if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let now = Self::step_at(unix_time);
        let mut accepted = None;
        for step in now.saturating_sub(1)..=now.saturating_add(1) {
            let matches: bool = self
                .code_at_step(step)
                .as_bytes()
                .ct_eq(code.as_bytes())
                .into();
            let fresh = last_accepted.is_none_or(|last| step > last);
            if matches && fresh && accepted.is_none() {
                accepted = Some(step);
            }
        }
        accepted
    }
}

fn hotp(key: &[u8], counter: u64, digits: u32) -> u32 {
    let mut mac =
        <Hmac<Sha1> as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[digest.len() - 1] & 0x0F);
    let truncated = u32::from_be_bytes([
        digest[offset] & 0x7F,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    truncated % 10_u32.pow(digits)
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn matches_the_rfc_6238_sha1_vectors() {
        let vectors = [
            (59_u64, 94_287_082_u32),
            (1_111_111_109, 7_081_804),
            (1_111_111_111, 14_050_471),
            (1_234_567_890, 89_005_924),
            (2_000_000_000, 69_279_037),
            (20_000_000_000, 65_353_130),
        ];
        for (time, expected) in vectors {
            assert_eq!(
                hotp(RFC_SECRET, time / STEP_SECONDS, 8),
                expected,
                "t={time}"
            );
        }
    }

    #[test]
    fn six_digit_codes_are_the_tail_of_the_eight_digit_ones_and_keep_leading_zeros() {
        let secret = TotpSecret::from_bytes(RFC_SECRET).unwrap();
        assert_eq!(secret.code_at_step(59 / STEP_SECONDS), "287082");
        assert_eq!(secret.code_at_step(1_111_111_109 / STEP_SECONDS), "081804");
    }

    #[test]
    fn accepts_the_current_and_adjacent_steps_only() {
        let secret = TotpSecret::from_bytes(RFC_SECRET).unwrap();
        let time = 1_111_111_109;
        let step = TotpSecret::step_at(time);
        for offset in [step - 1, step, step + 1] {
            assert_eq!(
                secret.verify(&secret.code_at_step(offset), time, None),
                Some(offset)
            );
        }
        for offset in [step - 2, step + 2] {
            assert_eq!(
                secret.verify(&secret.code_at_step(offset), time, None),
                None
            );
        }
    }

    #[test]
    fn a_step_cannot_be_used_twice_or_after_a_later_one() {
        let secret = TotpSecret::from_bytes(RFC_SECRET).unwrap();
        let time = 1_111_111_109;
        let step = TotpSecret::step_at(time);
        let code = secret.code_at_step(step);
        assert_eq!(secret.verify(&code, time, Some(step - 1)), Some(step));
        assert_eq!(secret.verify(&code, time, Some(step)), None);
        assert_eq!(secret.verify(&code, time, Some(step + 1)), None);
        let older = secret.code_at_step(step - 1);
        assert_eq!(secret.verify(&older, time, Some(step)), None);
    }

    #[test]
    fn rejects_malformed_codes() {
        let secret = TotpSecret::from_bytes(RFC_SECRET).unwrap();
        for code in [
            "",
            "12345",
            "1234567",
            "12a456",
            "１２３４５６",
            " 12345",
            "+12345",
        ] {
            assert_eq!(secret.verify(code, 59, None), None, "{code:?}");
        }
    }

    #[test]
    fn rejects_a_wrong_code() {
        let secret = TotpSecret::from_bytes(RFC_SECRET).unwrap();
        let right = secret.code_at_step(TotpSecret::step_at(59));
        let wrong = format!("{:06}", (right.parse::<u32>().unwrap() + 1) % 1_000_000);
        assert_eq!(secret.verify(&wrong, 59, None), None);
    }

    #[test]
    fn rejects_secrets_of_the_wrong_length() {
        assert_eq!(
            TotpSecret::from_bytes(&[1; 19]),
            Err(CryptoError::InvalidSecret {
                expected: 20,
                actual: 19
            })
        );
    }

    #[test]
    fn builds_a_provisioning_uri() {
        let secret = TotpSecret::from_bytes(RFC_SECRET).unwrap();
        assert_eq!(
            secret.otpauth_uri("Wirehaus", "casa pc@example.org"),
            "otpauth://totp/Wirehaus:casa%20pc%40example.org?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Wirehaus&algorithm=SHA1&digits=6&period=30"
        );
    }

    #[test]
    fn generated_secrets_differ_and_do_not_leak_through_debug() {
        let a = TotpSecret::generate().unwrap();
        let b = TotpSecret::generate().unwrap();
        assert_ne!(a, b);
        assert_eq!(format!("{a:?}"), "TotpSecret(..)");
    }
}
