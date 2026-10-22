use crate::error::CryptoError;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
    }
    out
}

pub fn decode(text: &str) -> Result<Vec<u8>, CryptoError> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for character in text.chars().filter(|c| !matches!(c, '-' | ' ')) {
        let value = symbol(character)?;
        buffer = (buffer << 5) | u32::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(
                u8::try_from((buffer >> bits) & 0xFF).map_err(|_| CryptoError::InvalidBase32)?,
            );
        }
    }
    if bits >= 5 || buffer & ((1 << bits) - 1) != 0 {
        return Err(CryptoError::InvalidBase32);
    }
    Ok(out)
}

fn symbol(character: char) -> Result<u8, CryptoError> {
    match character.to_ascii_uppercase() {
        c @ 'A'..='Z' => Ok(c as u8 - b'A'),
        c @ '2'..='7' => Ok(c as u8 - b'2' + 26),
        _ => Err(CryptoError::InvalidBase32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: [(&[u8], &str); 7] = [
        (b"", ""),
        (b"f", "MY"),
        (b"fo", "MZXQ"),
        (b"foo", "MZXW6"),
        (b"foob", "MZXW6YQ"),
        (b"fooba", "MZXW6YTB"),
        (b"foobar", "MZXW6YTBOI"),
    ];

    #[test]
    fn matches_the_rfc_4648_vectors() {
        for (bytes, text) in VECTORS {
            assert_eq!(encode(bytes), text);
            assert_eq!(decode(text).unwrap(), bytes);
        }
    }

    #[test]
    fn decoding_ignores_case_dashes_and_spaces() {
        assert_eq!(decode("mzxw-6ytb oi").unwrap(), b"foobar");
    }

    #[test]
    fn rejects_characters_outside_the_alphabet() {
        for text in ["MZXW1", "MZXW0", "MZXW8", "MZXW9", "MZX=", "MZXW6!", "ñ"] {
            assert_eq!(decode(text), Err(CryptoError::InvalidBase32), "{text}");
        }
    }

    #[test]
    fn rejects_lengths_the_rfc_does_not_allow() {
        for text in ["A", "MYA", "MZXW6YTBO", "MZXW6YTBOIA", "AAAAAAAAA"] {
            assert_eq!(decode(text), Err(CryptoError::InvalidBase32), "{text}");
        }
    }

    #[test]
    fn rejects_non_canonical_trailing_bits() {
        assert_eq!(decode("MZ"), Err(CryptoError::InvalidBase32));
        assert_eq!(decode("MY").unwrap(), b"f");
    }

    #[test]
    fn round_trips_every_length_up_to_forty_bytes() {
        let data: Vec<u8> = (0..40)
            .map(|i| u8::try_from((i * 7 + 3) % 256).unwrap())
            .collect();
        for length in 0..=data.len() {
            assert_eq!(decode(&encode(&data[..length])).unwrap(), &data[..length]);
        }
    }
}
