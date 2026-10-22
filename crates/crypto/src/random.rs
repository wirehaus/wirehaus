use crate::error::CryptoError;

pub fn fill(buffer: &mut [u8]) -> Result<(), CryptoError> {
    getrandom::fill(buffer).map_err(|_| CryptoError::Random)
}
