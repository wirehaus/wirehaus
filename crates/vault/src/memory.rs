use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::Vault;
use crate::error::VaultError;

#[derive(Debug, Clone, Default)]
pub struct MemoryVault {
    entries: Arc<Mutex<HashMap<String, String>>>,
}

impl MemoryVault {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Vault for MemoryVault {
    fn get(&self, name: &str) -> Result<Option<String>, VaultError> {
        Ok(self.lock().get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<(), VaultError> {
        self.lock().insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), VaultError> {
        self.lock().remove(name);
        Ok(())
    }
}
