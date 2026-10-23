use std::collections::HashMap;

use keyring_core::{Entry, Error};

use crate::error::VaultError;
use crate::{SERVICE, Vault};

#[derive(Debug)]
pub struct NativeVault {
    _store: (),
}

#[cfg(target_os = "macos")]
fn install_store() -> Result<(), VaultError> {
    let store =
        apple_native_keyring_store::keychain::Store::new().map_err(|error| unavailable(&error))?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(windows)]
fn install_store() -> Result<(), VaultError> {
    let store = windows_native_keyring_store::Store::new().map_err(|error| unavailable(&error))?;
    keyring_core::set_default_store(store);
    Ok(())
}

fn unavailable(error: &Error) -> VaultError {
    VaultError::Unavailable(error.to_string())
}

fn failed(error: &Error) -> VaultError {
    match error {
        Error::NoStorageAccess(_) | Error::NoDefaultStore => {
            VaultError::Unavailable(error.to_string())
        }
        other => VaultError::Failed(other.to_string()),
    }
}

impl NativeVault {
    pub fn open() -> Result<Self, VaultError> {
        install_store()?;
        Ok(Self { _store: () })
    }

    fn entry(name: &str) -> Result<Entry, VaultError> {
        #[cfg(windows)]
        let modifiers = HashMap::from([("persistence", "Local")]);
        #[cfg(not(windows))]
        let modifiers = HashMap::new();
        Entry::new_with_modifiers(SERVICE, name, &modifiers).map_err(|error| failed(&error))
    }
}

impl Vault for NativeVault {
    fn get(&self, name: &str) -> Result<Option<String>, VaultError> {
        match Self::entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(failed(&error)),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), VaultError> {
        Self::entry(name)?
            .set_password(value)
            .map_err(|error| failed(&error))
    }

    fn delete(&self, name: &str) -> Result<(), VaultError> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(failed(&error)),
        }
    }
}
