use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::state::{State, StateError};

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

const STATE_FILE: &str = "state";
const STATE_TEMP: &str = "state.tmp";
const LOCK_FILE: &str = "state.lock";
const LOG_FILE: &str = "access.log";
const LOG_ROTATED: &str = "access.log.1";
const MAX_LOG_BYTES: u64 = 1 << 20;
const MAX_LOG_DETAIL: usize = 200;
const LOCK_WAIT: Duration = Duration::from_secs(5);
const LOCK_STALE: Duration = Duration::from_secs(10);
const LOCK_POLL: Duration = Duration::from_millis(20);

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("{0} does not exist; run `wirehaus-agent init` first")]
    NotInitialized(PathBuf),
    #[error("{0} already holds an agent identity")]
    AlreadyInitialized(PathBuf),
    #[error("{0} can be read by other users; it must be private to its owner")]
    InsecurePermissions(PathBuf),
    #[error("the state is locked by another process")]
    LockTimeout,
    #[error("cannot access the state: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    State(#[from] StateError),
}

struct Cached {
    state: State,
    text: String,
}

pub struct Store {
    dir: PathBuf,
    clock: Clock,
    cached: Mutex<Cached>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
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
            cached: Mutex::new(Cached { state, text }),
        })
    }

    pub fn open(dir: &Path, clock: Clock) -> Result<Self, StoreError> {
        let state_path = dir.join(STATE_FILE);
        if !state_path.exists() {
            return Err(StoreError::NotInitialized(dir.to_path_buf()));
        }
        ensure_private(dir)?;
        ensure_private(&state_path)?;
        let text = fs::read_to_string(&state_path)?;
        let state = State::parse(&text)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            clock,
            cached: Mutex::new(Cached { state, text }),
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

    pub fn log(&self, event: &str, detail: &str) {
        let line = format!(
            "{} {} {}\n",
            self.now(),
            clean(event, 40),
            clean(detail, MAX_LOG_DETAIL)
        );
        let _ = self.append_log(&line);
    }

    fn refresh(&self, cached: &mut Cached) -> Result<(), StoreError> {
        let path = self.dir.join(STATE_FILE);
        ensure_private(&path)?;
        let text = fs::read_to_string(&path)?;
        if text != cached.text {
            cached.state = State::parse(&text)?;
            cached.text = text;
        }
        Ok(())
    }

    fn append_log(&self, line: &str) -> std::io::Result<()> {
        let path = self.dir.join(LOG_FILE);
        if fs::metadata(&path).is_ok_and(|meta| meta.len() >= MAX_LOG_BYTES) {
            fs::rename(&path, self.dir.join(LOG_ROTATED))?;
        }
        let mut file = private_options().append(true).create(true).open(path)?;
        file.write_all(line.as_bytes())
    }
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
    Ok(())
}

fn private_options() -> OpenOptions {
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
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

struct Lock {
    path: PathBuf,
}

impl Lock {
    fn acquire(dir: &Path) -> Result<Self, StoreError> {
        let path = dir.join(LOCK_FILE);
        let mut waited = Duration::ZERO;
        loop {
            match private_options().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    drop::<File>(file);
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    if is_stale(&path) {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if waited >= LOCK_WAIT {
                        return Err(StoreError::LockTimeout);
                    }
                    thread::sleep(LOCK_POLL);
                    waited += LOCK_POLL;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age > LOCK_STALE)
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
        assert!(!dir.0.join(LOCK_FILE).exists());
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
    fn a_stale_lock_from_a_dead_process_is_broken() {
        let dir = TempDir::new();
        let (clock, _) = clock(0);
        let store = Store::initialize(&dir.0, clock).unwrap();
        let lock = dir.0.join(LOCK_FILE);
        fs::write(&lock, b"").unwrap();
        let old = SystemTime::now() - LOCK_STALE - Duration::from_secs(5);
        File::options()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(store.update(|_, _| ()).is_ok());
        assert!(!lock.exists());
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
        assert!(dir.0.join(LOG_ROTATED).exists());
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
