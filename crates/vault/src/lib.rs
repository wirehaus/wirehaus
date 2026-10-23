mod error;
mod memory;
#[cfg(any(target_os = "macos", windows))]
mod native;
#[cfg(any(target_os = "linux", all(test, unix)))]
mod tool;

pub use error::VaultError;
pub use memory::MemoryVault;
#[cfg(any(target_os = "macos", windows))]
pub use native::NativeVault;
#[cfg(any(target_os = "linux", all(test, unix)))]
pub use tool::ToolVault;

pub const SERVICE: &str = "wirehaus-agent";

const PROBE_NAME: &str = "probe";
const PROBE_VALUE: &str = "wirehaus-vault-probe";

pub trait Vault: Send + Sync + std::fmt::Debug {
    fn get(&self, name: &str) -> Result<Option<String>, VaultError>;
    fn set(&self, name: &str, value: &str) -> Result<(), VaultError>;
    fn delete(&self, name: &str) -> Result<(), VaultError>;
}

pub fn check(vault: &dyn Vault) -> Result<(), VaultError> {
    vault.set(PROBE_NAME, PROBE_VALUE)?;
    let read = vault.get(PROBE_NAME)?;
    vault.delete(PROBE_NAME)?;
    if read.as_deref() == Some(PROBE_VALUE) {
        Ok(())
    } else {
        Err(VaultError::Mismatch)
    }
}

#[cfg(target_os = "linux")]
pub fn system_vault() -> Result<Box<dyn Vault>, VaultError> {
    Ok(Box::new(ToolVault::secret_tool()))
}

#[cfg(any(target_os = "macos", windows))]
pub fn system_vault() -> Result<Box<dyn Vault>, VaultError> {
    Ok(Box::new(NativeVault::open()?))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn system_vault() -> Result<Box<dyn Vault>, VaultError> {
    Err(VaultError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Forgetful;

    impl Vault for Forgetful {
        fn get(&self, _name: &str) -> Result<Option<String>, VaultError> {
            Ok(None)
        }

        fn set(&self, _name: &str, _value: &str) -> Result<(), VaultError> {
            Ok(())
        }

        fn delete(&self, _name: &str) -> Result<(), VaultError> {
            Ok(())
        }
    }

    #[test]
    fn a_working_vault_passes_the_check_and_keeps_nothing() {
        let vault = MemoryVault::new();
        check(&vault).unwrap();
        assert!(vault.is_empty());
    }

    #[test]
    fn a_vault_that_forgets_what_it_is_given_fails_the_check() {
        assert_eq!(check(&Forgetful), Err(VaultError::Mismatch));
    }

    #[test]
    fn copies_of_the_memory_vault_share_what_they_hold() {
        let vault = MemoryVault::new();
        let copy = vault.clone();
        vault.set("a", "1").unwrap();
        assert_eq!(copy.get("a").unwrap().as_deref(), Some("1"));
    }

    #[test]
    fn the_memory_vault_stores_replaces_and_removes() {
        let vault = MemoryVault::new();
        assert_eq!(vault.get("a").unwrap(), None);
        vault.set("a", "1").unwrap();
        vault.set("a", "2").unwrap();
        assert_eq!(vault.get("a").unwrap().as_deref(), Some("2"));
        assert_eq!(vault.len(), 1);
        vault.delete("a").unwrap();
        vault.delete("a").unwrap();
        assert_eq!(vault.get("a").unwrap(), None);
    }
}
