use std::collections::HashMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use thiserror::Error;
use wirehaus_crypto::base32;
use wirehaus_vault::{Vault, VaultError};

use crate::i18n::tr;
use crate::state::{SecretStorage, Secrets, State, StateError};

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;
pub type VaultSource = Arc<dyn Fn() -> Result<Box<dyn Vault>, VaultError> + Send + Sync>;

const STATE_FILE: &str = "state";
const STATE_TEMP: &str = "state.tmp";
const LOCK_FILE: &str = "state.lock";
const LOG_FILE: &str = "access.log";
const MAX_LOG_BYTES: u64 = 1 << 20;
const LOG_COPIES: u32 = 5;
const NOISY_WINDOW: u64 = 10;
const NOISY_EVENTS: [&str; 4] = [
    "handshake_failed",
    "unknown_device",
    "refused_locked",
    "pairing_failed",
];
const MAX_LOG_DETAIL: usize = 200;
const LOCK_WAIT: Duration = Duration::from_secs(5);
const LOCK_POLL: Duration = Duration::from_millis(20);

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("{}", self.describe())]
    NotInitialized(PathBuf),
    #[error("{}", self.describe())]
    AlreadyInitialized(PathBuf),
    #[error("{}", self.describe())]
    InsecurePermissions(PathBuf),
    #[error("{}", self.describe())]
    LockTimeout,
    #[error("{}", self.describe())]
    Io(#[from] std::io::Error),
    #[error("{}", self.describe())]
    Vault(#[from] VaultError),
    #[error("{}", self.describe())]
    SecretsNotInVault,
    #[error(transparent)]
    State(#[from] StateError),
}

impl StoreError {
    fn describe(&self) -> String {
        match self {
            StoreError::NotInitialized(path) => tr!(
                format!(
                    "{} no existe; ejecuta primero `wirehaus-agent init`",
                    path.display()
                ),
                format!(
                    "{} does not exist; run `wirehaus-agent init` first",
                    path.display()
                )
            ),
            StoreError::AlreadyInitialized(path) => tr!(
                format!("{} ya contiene una identidad del agente", path.display()),
                format!("{} already holds an agent identity", path.display())
            ),
            StoreError::InsecurePermissions(path) => tr!(
                format!(
                    "{} lo pueden leer otros usuarios; debe ser privado de su dueño",
                    path.display()
                ),
                format!(
                    "{} can be read by other users; it must be private to its owner",
                    path.display()
                )
            ),
            StoreError::LockTimeout => tr!(
                "el estado está bloqueado por otro proceso",
                "the state is locked by another process"
            ),
            StoreError::Io(error) => tr!(
                format!("no se puede acceder al estado: {error}"),
                format!("cannot access the state: {error}")
            ),
            StoreError::Vault(error) => tr!(
                format!("almacén de claves del sistema: {error}"),
                format!("system vault: {error}")
            ),
            StoreError::SecretsNotInVault => tr!(
                "los secretos del agente no están en el almacén de claves del sistema; sin ellos hay que volver a ejecutar `wirehaus-agent init` y emparejar de nuevo",
                "the agent secrets are not in the system vault; without them you must run `wirehaus-agent init` again and pair again"
            ),
            StoreError::State(error) => error.to_string(),
        }
    }
}

struct Cached {
    state: State,
    text: String,
}

struct Quiet {
    last_written: u64,
    suppressed: u64,
}

pub struct Store {
    dir: PathBuf,
    clock: Clock,
    vault: VaultSource,
    cached: Mutex<Cached>,
    quiet: Mutex<HashMap<String, Quiet>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

#[must_use]
pub fn system_vault() -> VaultSource {
    Arc::new(wirehaus_vault::system_vault)
}

#[must_use]
pub fn system_clock() -> Clock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    })
}

#[must_use]
pub fn default_dir() -> PathBuf {
    dir_from(|name| std::env::var_os(name))
}

fn dir_from(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> PathBuf {
    if let Some(dir) = get("WIREHAUS_STATE_DIR") {
        return PathBuf::from(dir);
    }
    let base = get("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| get("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("wirehaus")
}

impl Store {
    pub fn initialize(dir: &Path, clock: Clock) -> Result<Self, StoreError> {
        let state_path = dir.join(STATE_FILE);
        if state_path.exists() {
            return Err(StoreError::AlreadyInitialized(state_path));
        }
        create_private_dir(dir)?;
        let _guard = Lock::acquire(dir)?;
        if state_path.exists() {
            return Err(StoreError::AlreadyInitialized(state_path));
        }
        let state = State::create()?;
        let text = state.serialize();
        write_text(dir, &text)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            clock,
            vault: system_vault(),
            cached: Mutex::new(Cached { state, text }),
            quiet: Mutex::new(HashMap::new()),
        })
    }

    pub fn open(dir: &Path, clock: Clock) -> Result<Self, StoreError> {
        Self::open_with(dir, clock, system_vault())
    }

    pub fn open_with(dir: &Path, clock: Clock, vault: VaultSource) -> Result<Self, StoreError> {
        let state_path = dir.join(STATE_FILE);
        if !state_path.exists() {
            return Err(StoreError::NotInitialized(dir.to_path_buf()));
        }
        ensure_private(dir)?;
        ensure_private(&state_path)?;
        let text = fs::read_to_string(&state_path)?;
        let (storage, public) = State::peek(&text)?;
        let secrets = match storage {
            SecretStorage::File => None,
            SecretStorage::System => Some(read_secrets(&vault, &public)?),
        };
        let state = State::parse_with(&text, secrets.as_ref())?;
        Ok(Self {
            dir: dir.to_path_buf(),
            clock,
            vault,
            cached: Mutex::new(Cached { state, text }),
            quiet: Mutex::new(HashMap::new()),
        })
    }

    #[must_use]
    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    pub fn read<R>(&self, f: impl FnOnce(&State, u64) -> R) -> Result<R, StoreError> {
        let mut cached = self.cached.lock().unwrap_or_else(PoisonError::into_inner);
        self.refresh(&mut cached)?;
        Ok(f(&cached.state, self.now()))
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut State, u64) -> R) -> Result<R, StoreError> {
        let mut cached = self.cached.lock().unwrap_or_else(PoisonError::into_inner);
        let _guard = Lock::acquire(&self.dir)?;
        self.refresh(&mut cached)?;
        let result = f(&mut cached.state, self.now());
        let text = cached.state.serialize();
        write_text(&self.dir, &text)?;
        cached.text = text;
        Ok(result)
    }

    pub fn storage(&self) -> Result<SecretStorage, StoreError> {
        self.read(|state, _| state.storage())
    }

    pub fn move_secrets(&self, target: SecretStorage) -> Result<bool, StoreError> {
        let mut cached = self.cached.lock().unwrap_or_else(PoisonError::into_inner);
        let _guard = Lock::acquire(&self.dir)?;
        self.refresh(&mut cached)?;
        if cached.state.storage() == target {
            return Ok(false);
        }
        let secrets = cached.state.secrets();
        let public = cached.state.agent_key().public();
        let vault = (self.vault)()?;
        if target == SecretStorage::System {
            wirehaus_vault::check(vault.as_ref())?;
            write_secrets(vault.as_ref(), &public, &secrets)?;
            if read_secrets_from(vault.as_ref(), &public)? != secrets {
                return Err(VaultError::Mismatch.into());
            }
        }
        cached.state.set_storage(target);
        let text = cached.state.serialize();
        write_text(&self.dir, &text)?;
        cached.text = text;
        if target == SecretStorage::File {
            let (private, totp) = Secrets::names(&public);
            vault.delete(&private)?;
            vault.delete(&totp)?;
        }
        Ok(true)
    }

    pub fn log(&self, event: &str, detail: &str) {
        let now = self.now();
        let event = clean(event, 40);
        let mut detail = clean(detail, MAX_LOG_DETAIL);
        if NOISY_EVENTS.contains(&event.as_str()) {
            let mut quiet = self.quiet.lock().unwrap_or_else(PoisonError::into_inner);
            let entry = quiet.entry(event.clone()).or_insert(Quiet {
                last_written: 0,
                suppressed: 0,
            });
            if entry.last_written != 0 && now.saturating_sub(entry.last_written) < NOISY_WINDOW {
                entry.suppressed += 1;
                return;
            }
            if entry.suppressed > 0 {
                detail = format!("{detail} (+{} similar events suppressed)", entry.suppressed);
            }
            entry.last_written = now;
            entry.suppressed = 0;
        }
        let _ = self.append_log(&format!("{now} {event} {detail}\n"));
    }

    fn refresh(&self, cached: &mut Cached) -> Result<(), StoreError> {
        let path = self.dir.join(STATE_FILE);
        ensure_private(&path)?;
        let text = fs::read_to_string(&path)?;
        if text != cached.text {
            cached.state = State::parse_with(&text, Some(&cached.state.secrets()))?;
            cached.text = text;
        }
        Ok(())
    }

    fn append_log(&self, line: &str) -> std::io::Result<()> {
        let path = self.dir.join(LOG_FILE);
        if fs::metadata(&path).is_ok_and(|meta| meta.len() >= MAX_LOG_BYTES) {
            self.rotate_log()?;
        }
        let mut file = private_options().append(true).create(true).open(path)?;
        file.write_all(line.as_bytes())
    }

    fn rotate_log(&self) -> std::io::Result<()> {
        let numbered = |number: u32| self.dir.join(format!("{LOG_FILE}.{number}"));
        let _ = fs::remove_file(numbered(LOG_COPIES));
        for number in (1..LOG_COPIES).rev() {
            if numbered(number).exists() {
                fs::rename(numbered(number), numbered(number + 1))?;
            }
        }
        fs::rename(self.dir.join(LOG_FILE), numbered(1))
    }
}

fn read_secrets(
    source: &VaultSource,
    public: &wirehaus_crypto::keys::PublicKey,
) -> Result<Secrets, StoreError> {
    let vault = source()?;
    read_secrets_from(vault.as_ref(), public)
}

fn read_secrets_from(
    vault: &dyn Vault,
    public: &wirehaus_crypto::keys::PublicKey,
) -> Result<Secrets, StoreError> {
    let (private, totp) = Secrets::names(public);
    let decode = |name: &str| -> Result<Vec<u8>, StoreError> {
        let text = vault.get(name)?.ok_or(StoreError::SecretsNotInVault)?;
        base32::decode(text.trim()).map_err(|_| StoreError::SecretsNotInVault)
    };
    Ok(Secrets {
        agent_private: decode(&private)?,
        totp_secret: decode(&totp)?,
    })
}

fn write_secrets(
    vault: &dyn Vault,
    public: &wirehaus_crypto::keys::PublicKey,
    secrets: &Secrets,
) -> Result<(), StoreError> {
    let (private, totp) = Secrets::names(public);
    vault.set(&private, &base32::encode(&secrets.agent_private))?;
    vault.set(&totp, &base32::encode(&secrets.totp_secret))?;
    Ok(())
}

fn clean(text: &str, max: usize) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect()
}

fn write_text(dir: &Path, text: &str) -> Result<(), StoreError> {
    let temp = dir.join(STATE_TEMP);
    let mut file = private_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, dir.join(STATE_FILE))?;
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

fn private_options() -> OpenOptions {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn create_private_dir(dir: &Path) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(dir)?;
    ensure_private(dir)
}

fn ensure_private(path: &Path) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
            return Err(StoreError::InsecurePermissions(path.to_path_buf()));
        }
    }
    #[cfg(windows)]
    crate::winacl::restrict_to_current_user(path)
        .map_err(|_| StoreError::InsecurePermissions(path.to_path_buf()))?;
    #[cfg(not(any(unix, windows)))]
    let _ = path;
    Ok(())
}

struct Lock {
    file: File,
}

impl Lock {
    fn acquire(dir: &Path) -> Result<Self, StoreError> {
        let file = private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(LOCK_FILE))?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { file }),
                Err(TryLockError::WouldBlock) => {
                    if started.elapsed() >= LOCK_WAIT {
                        return Err(StoreError::LockTimeout);
                    }
                    thread::sleep(LOCK_POLL);
                }
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "wirehaus-store-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            ));
            Self(path.join("agent"))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            if let Some(parent) = self.0.parent() {
                let _ = fs::remove_dir_all(parent);
            }
        }
    }

    fn clock(start: u64) -> (Clock, Arc<AtomicU64>) {
        let time = Arc::new(AtomicU64::new(start));
        let shared = Arc::clone(&time);
        (Arc::new(move || shared.load(Ordering::SeqCst)), time)
    }

    #[test]
    fn initializing_creates_a_private_state_directory_and_file() {
        let dir = TempDir::new();
        let (clock, _) = clock(100);
        Store::initialize(&dir.0, clock).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(dir.0.join(STATE_FILE))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        #[cfg(windows)]
        assert_eq!(crate::winacl::grantees(&dir.0).unwrap().len(), 2);
        assert!(!dir.0.join(STATE_TEMP).exists());
    }

    #[test]
    fn initializing_twice_is_refused() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        assert!(matches!(
            Store::initialize(&dir.0, clock),
            Err(StoreError::AlreadyInitialized(_))
        ));
    }

    #[test]
    fn opening_a_missing_state_asks_to_initialize() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        assert!(matches!(
            Store::open(&dir.0, clock),
            Err(StoreError::NotInitialized(_))
        ));
    }

    #[test]
    fn a_reopened_store_sees_what_was_saved() {
        let dir = TempDir::new();
        let (clock, _) = clock(500);
        let store = Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        let public = store.read(|state, _| state.agent_key().public()).unwrap();
        store
            .update(|state, now| state.open_pairing(now).map(drop))
            .unwrap()
            .unwrap();
        let reopened = Store::open(&dir.0, clock).unwrap();
        assert_eq!(
            reopened
                .read(|state, _| state.agent_key().public())
                .unwrap(),
            public
        );
        assert!(reopened.read(State::pairing_psk).unwrap().is_some());
    }

    #[test]
    fn a_change_made_by_another_process_is_picked_up() {
        let dir = TempDir::new();
        let (clock, _) = clock(500);
        let server = Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        assert!(server.read(State::pairing_psk).unwrap().is_none());
        let cli = Store::open(&dir.0, clock).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        cli.update(|state, now| state.open_pairing(now).map(drop))
            .unwrap()
            .unwrap();
        assert!(server.read(State::pairing_psk).unwrap().is_some());
        server.update(|state, _| state.set_locked(true)).unwrap();
        assert!(cli.read(|state, _| state.is_locked()).unwrap());
    }

    #[test]
    fn updates_use_the_injected_clock() {
        let dir = TempDir::new();
        let (clock, time) = clock(1000);
        let store = Store::initialize(&dir.0, clock).unwrap();
        assert_eq!(store.update(|_, now| now).unwrap(), 1000);
        time.store(2000, Ordering::SeqCst);
        assert_eq!(store.read(|_, now| now).unwrap(), 2000);
    }

    #[test]
    fn a_state_readable_by_others_is_refused() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.0.join(STATE_FILE), fs::Permissions::from_mode(0o644)).unwrap();
            assert!(matches!(
                Store::open(&dir.0, Arc::clone(&clock)),
                Err(StoreError::InsecurePermissions(_))
            ));
            fs::set_permissions(dir.0.join(STATE_FILE), fs::Permissions::from_mode(0o600)).unwrap();
            fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(matches!(
                Store::open(&dir.0, clock),
                Err(StoreError::InsecurePermissions(_))
            ));
        }
    }

    #[test]
    fn a_corrupted_state_is_an_error_not_a_reset() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        fs::write(dir.0.join(STATE_FILE), "garbage").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.0.join(STATE_FILE), fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(matches!(
            Store::open(&dir.0, clock),
            Err(StoreError::State(_))
        ));
    }

    fn memory_source(vault: &wirehaus_vault::MemoryVault) -> VaultSource {
        let vault = vault.clone();
        Arc::new(move || Ok(Box::new(vault.clone()) as Box<dyn Vault>))
    }

    #[derive(Debug)]
    struct Broken;

    impl Vault for Broken {
        fn get(&self, _name: &str) -> Result<Option<String>, VaultError> {
            Err(VaultError::Unavailable("locked".to_owned()))
        }

        fn set(&self, _name: &str, _value: &str) -> Result<(), VaultError> {
            Err(VaultError::Unavailable("locked".to_owned()))
        }

        fn delete(&self, _name: &str) -> Result<(), VaultError> {
            Err(VaultError::Unavailable("locked".to_owned()))
        }
    }

    fn private_key_text(store: &Store) -> String {
        store
            .read(|state, _| base32::encode(state.agent_key().private_bytes()))
            .unwrap()
    }

    #[test]
    fn moving_the_secrets_to_the_vault_takes_them_out_of_the_file() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        let vault = wirehaus_vault::MemoryVault::new();
        Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        let store = Store::open_with(&dir.0, clock.clone(), memory_source(&vault)).unwrap();
        let private = private_key_text(&store);
        let totp = store.read(|state, _| state.totp().to_base32()).unwrap();
        assert!(store.move_secrets(SecretStorage::System).unwrap());
        let file = fs::read_to_string(dir.0.join(STATE_FILE)).unwrap();
        assert!(!file.contains(&private) && !file.contains(&totp), "{file}");
        assert!(file.contains("secrets system"));
        assert_eq!(vault.len(), 2);
        assert_eq!(store.storage().unwrap(), SecretStorage::System);

        let reopened = Store::open_with(&dir.0, clock, memory_source(&vault)).unwrap();
        assert_eq!(reopened.storage().unwrap(), SecretStorage::System);
        assert_eq!(private_key_text(&reopened), private);
        assert_eq!(
            reopened.read(|state, _| state.totp().to_base32()).unwrap(),
            totp
        );
    }

    #[test]
    fn moving_them_back_restores_the_file_and_empties_the_vault() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        let vault = wirehaus_vault::MemoryVault::new();
        Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        let store = Store::open_with(&dir.0, clock.clone(), memory_source(&vault)).unwrap();
        let private = private_key_text(&store);
        store.move_secrets(SecretStorage::System).unwrap();
        assert!(store.move_secrets(SecretStorage::File).unwrap());
        assert!(vault.is_empty());
        let file = fs::read_to_string(dir.0.join(STATE_FILE)).unwrap();
        assert!(file.contains(&private));
        assert!(!file.contains("secrets system"));
        let plain =
            Store::open_with(&dir.0, clock, Arc::new(|| Err(VaultError::Unsupported))).unwrap();
        assert_eq!(private_key_text(&plain), private);
    }

    #[test]
    fn moving_to_where_they_already_are_does_nothing() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        let vault = wirehaus_vault::MemoryVault::new();
        Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        let store = Store::open_with(&dir.0, clock, memory_source(&vault)).unwrap();
        assert!(!store.move_secrets(SecretStorage::File).unwrap());
        assert!(vault.is_empty());
        assert!(store.move_secrets(SecretStorage::System).unwrap());
        assert!(!store.move_secrets(SecretStorage::System).unwrap());
    }

    #[test]
    fn a_vault_that_does_not_work_leaves_the_file_untouched() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        let store = Store::open_with(
            &dir.0,
            clock,
            Arc::new(|| Ok(Box::new(Broken) as Box<dyn Vault>)),
        )
        .unwrap();
        let before = fs::read_to_string(dir.0.join(STATE_FILE)).unwrap();
        assert!(matches!(
            store.move_secrets(SecretStorage::System),
            Err(StoreError::Vault(_))
        ));
        assert_eq!(fs::read_to_string(dir.0.join(STATE_FILE)).unwrap(), before);
        assert_eq!(store.storage().unwrap(), SecretStorage::File);
    }

    #[test]
    fn opening_with_the_secrets_missing_from_the_vault_is_a_clear_error() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        let vault = wirehaus_vault::MemoryVault::new();
        Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        Store::open_with(&dir.0, Arc::clone(&clock), memory_source(&vault))
            .unwrap()
            .move_secrets(SecretStorage::System)
            .unwrap();
        let empty = wirehaus_vault::MemoryVault::new();
        assert!(matches!(
            Store::open_with(&dir.0, Arc::clone(&clock), memory_source(&empty)),
            Err(StoreError::SecretsNotInVault)
        ));
        assert!(matches!(
            Store::open_with(
                &dir.0,
                clock,
                Arc::new(|| Ok(Box::new(Broken) as Box<dyn Vault>))
            ),
            Err(StoreError::Vault(_))
        ));
    }

    #[test]
    fn a_running_store_follows_another_process_that_moves_the_secrets() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        let vault = wirehaus_vault::MemoryVault::new();
        Store::initialize(&dir.0, Arc::clone(&clock)).unwrap();
        let running = Store::open_with(&dir.0, Arc::clone(&clock), memory_source(&vault)).unwrap();
        let private = private_key_text(&running);
        let cli = Store::open_with(&dir.0, clock, memory_source(&vault)).unwrap();
        cli.move_secrets(SecretStorage::System).unwrap();
        assert_eq!(running.storage().unwrap(), SecretStorage::System);
        assert_eq!(private_key_text(&running), private);
        cli.move_secrets(SecretStorage::File).unwrap();
        assert_eq!(running.storage().unwrap(), SecretStorage::File);
        assert_eq!(private_key_text(&running), private);
    }

    #[test]
    fn a_lock_held_by_someone_else_times_out() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        let store = Store::initialize(&dir.0, clock).unwrap();
        let _held = Lock::acquire(&dir.0).unwrap();
        let start = std::time::Instant::now();
        assert!(matches!(
            store.update(|_, _| ()),
            Err(StoreError::LockTimeout)
        ));
        assert!(start.elapsed() >= LOCK_WAIT);
    }

    #[test]
    fn a_lock_is_free_again_as_soon_as_its_holder_lets_go() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        let store = Store::initialize(&dir.0, clock).unwrap();
        drop(Lock::acquire(&dir.0).unwrap());
        let start = std::time::Instant::now();
        assert!(store.update(|_, _| ()).is_ok());
        assert!(start.elapsed() < LOCK_WAIT);
    }

    #[test]
    fn a_leftover_lock_file_does_not_block_anyone() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        let store = Store::initialize(&dir.0, clock).unwrap();
        fs::write(dir.0.join(LOCK_FILE), b"left by a process that died").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.0.join(LOCK_FILE), fs::Permissions::from_mode(0o600)).unwrap();
        }
        let start = std::time::Instant::now();
        assert!(store.update(|_, _| ()).is_ok());
        assert!(start.elapsed() < LOCK_WAIT);
    }

    #[test]
    fn concurrent_updates_from_many_threads_are_not_lost() {
        let dir = TempDir::new();
        let (clock, _) = clock(1000);
        let store = Arc::new(Store::initialize(&dir.0, clock).unwrap());
        let handles: Vec<_> = (0..6)
            .map(|index| {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    let key = wirehaus_crypto::keys::StaticKey::generate()
                        .unwrap()
                        .public();
                    store
                        .update(|state, now| state.add_device(key, &format!("dev {index}"), now))
                        .unwrap()
                        .unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(store.read(|state, _| state.devices().len()).unwrap(), 6);
        let reopened = Store::open(&dir.0, clock_of(1000)).unwrap();
        assert_eq!(reopened.read(|state, _| state.devices().len()).unwrap(), 6);
    }

    fn clock_of(start: u64) -> Clock {
        clock(start).0
    }

    #[test]
    fn the_access_log_records_events_without_control_characters() {
        let dir = TempDir::new();
        let (clock, _) = clock(1234);
        let store = Store::initialize(&dir.0, clock).unwrap();
        store.log("totp_failed", "device Aula 3\nforged 9999 totp_ok injected");
        store.log("connect", &"x".repeat(1000));
        let text = fs::read_to_string(dir.0.join(LOG_FILE)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("1234 totp_failed device Aula 3 forged"));
        assert!(lines[1].len() <= "1234 connect ".len() + MAX_LOG_DETAIL);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(dir.0.join(LOG_FILE))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn events_anyone_can_trigger_are_rate_limited_and_counted() {
        let dir = TempDir::new();
        let (clock, time) = clock(1000);
        let store = Store::initialize(&dir.0, clock).unwrap();
        for _ in 0..50 {
            store.log("unknown_device", "ABC");
        }
        store.log("auth_ok", "Aula 3");
        time.store(1000 + NOISY_WINDOW, Ordering::SeqCst);
        store.log("unknown_device", "XYZ");
        let text = fs::read_to_string(dir.0.join(LOG_FILE)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert!(lines[0].ends_with("unknown_device ABC"));
        assert!(lines[1].contains("auth_ok"));
        assert!(lines[2].contains("XYZ (+49 similar events suppressed)"));
    }

    #[test]
    fn a_flood_of_anonymous_events_cannot_push_real_events_out_of_the_log() {
        let dir = TempDir::new();
        let (clock, time) = clock(5000);
        let store = Store::initialize(&dir.0, clock).unwrap();
        store.log("revoked", "Dispositivo importante");
        for second in 0..20_000_u64 {
            time.store(5000 + second * NOISY_WINDOW, Ordering::SeqCst);
            store.log("unknown_device", &"K".repeat(MAX_LOG_DETAIL));
        }
        let mut all = String::new();
        for name in [
            "access.log",
            "access.log.1",
            "access.log.2",
            "access.log.3",
            "access.log.4",
            "access.log.5",
        ] {
            all.push_str(&fs::read_to_string(dir.0.join(name)).unwrap_or_default());
        }
        assert!(all.contains("revoked Dispositivo importante"));
    }

    #[test]
    fn auditing_events_are_never_dropped() {
        let dir = TempDir::new();
        let (clock, _) = clock(10);
        let store = Store::initialize(&dir.0, clock).unwrap();
        for _ in 0..5 {
            store.log("totp_failed", "Aula 3");
        }
        let text = fs::read_to_string(dir.0.join(LOG_FILE)).unwrap();
        assert_eq!(text.lines().count(), 5);
    }

    #[test]
    fn the_access_log_is_rotated_when_it_grows() {
        let dir = TempDir::new();
        let (clock, _) = clock(1);
        let store = Store::initialize(&dir.0, clock).unwrap();
        fs::write(
            dir.0.join(LOG_FILE),
            vec![b'a'; usize::try_from(MAX_LOG_BYTES).unwrap()],
        )
        .unwrap();
        store.log("connect", "after the rotation");
        assert!(dir.0.join(format!("{LOG_FILE}.1")).exists());
        let text = fs::read_to_string(dir.0.join(LOG_FILE)).unwrap();
        assert_eq!(text, "1 connect after the rotation\n");
    }

    fn environment(
        pairs: &[(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Option<std::ffi::OsString> {
        let pairs = pairs.to_vec();
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| std::ffi::OsString::from(value))
        }
    }

    #[test]
    fn the_state_directory_follows_the_environment() {
        assert_eq!(
            dir_from(environment(&[
                ("WIREHAUS_STATE_DIR", "/srv/w"),
                ("HOME", "/home/a")
            ])),
            PathBuf::from("/srv/w")
        );
        assert_eq!(
            dir_from(environment(&[
                ("XDG_CONFIG_HOME", "/cfg"),
                ("HOME", "/home/a")
            ])),
            PathBuf::from("/cfg/wirehaus")
        );
        assert_eq!(
            dir_from(environment(&[("HOME", "/home/a")])),
            PathBuf::from("/home/a/.config/wirehaus")
        );
        assert_eq!(dir_from(environment(&[])), PathBuf::from("./wirehaus"));
    }
}
