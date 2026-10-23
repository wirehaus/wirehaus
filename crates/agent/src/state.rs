use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use thiserror::Error;
use wirehaus_crypto::CryptoError;
use wirehaus_crypto::base32;
use wirehaus_crypto::handshake::validate_device_name;
use wirehaus_crypto::keys::{PublicKey, StaticKey};
use wirehaus_crypto::lockout::{Denied, FailureTracker, MAX_LOCK_SECONDS, SUSPEND_AT};
use wirehaus_crypto::pairing::{PSK_BYTES, PairingCode, PairingGrant};
use wirehaus_crypto::totp::TotpSecret;

use crate::i18n::tr;

pub const MAX_DEVICES: usize = 32;
pub const MAX_STATE_BYTES: usize = 64 * 1024;
pub const MAX_FILE_ROOTS: usize = 16;

const HEADER: &str = "wirehaus-state 1";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StateError {
    #[error("{}", self.describe())]
    Invalid(String),
    #[error("{}", self.describe())]
    TooLarge,
    #[error("{}", self.describe())]
    DuplicateDevice,
    #[error("{}", self.describe())]
    TooManyDevices,
    #[error("{}", self.describe())]
    TooManyRoots,
    #[error("{}", self.describe())]
    NoSuchRoot,
    #[error("{}", self.describe())]
    NoSuchDevice,
    #[error("{}", self.describe())]
    AmbiguousDevice,
    #[error("{}", self.describe())]
    SecretsMissing,
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

impl StateError {
    fn describe(&self) -> String {
        match self {
            StateError::Invalid(reason) => tr!(
                format!("el fichero de estado no es válido: {reason}"),
                format!("the state file is not valid: {reason}")
            ),
            StateError::TooLarge => tr!(
                format!("el fichero de estado supera {MAX_STATE_BYTES} bytes"),
                format!("the state file is larger than {MAX_STATE_BYTES} bytes")
            ),
            StateError::DuplicateDevice => tr!(
                "ya hay un dispositivo con esa clave",
                "there is already a device with that key"
            ),
            StateError::TooManyDevices => tr!(
                format!("se ha alcanzado el límite de {MAX_DEVICES} dispositivos"),
                format!("the limit of {MAX_DEVICES} devices has been reached")
            ),
            StateError::TooManyRoots => tr!(
                format!("se ha alcanzado el límite de {MAX_FILE_ROOTS} carpetas compartidas"),
                format!("the limit of {MAX_FILE_ROOTS} shared folders has been reached")
            ),
            StateError::NoSuchRoot => tr!(
                "esa carpeta no está compartida",
                "that folder is not shared"
            ),
            StateError::NoSuchDevice => tr!("ningún dispositivo coincide", "no device matches"),
            StateError::AmbiguousDevice => tr!(
                "más de un dispositivo coincide",
                "more than one device matches"
            ),
            StateError::SecretsMissing => tr!(
                "el estado guarda sus secretos en el almacén del sistema y no se han podido leer",
                "the state keeps its secrets in the system vault and they could not be read"
            ),
            StateError::Crypto(error) => error.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    WrongCode,
    Denied(Denied),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipboardMode {
    #[default]
    Off,
    ToRemote,
    FromRemote,
    Both,
}

impl ClipboardMode {
    #[must_use]
    pub fn allows_write(self) -> bool {
        matches!(self, ClipboardMode::ToRemote | ClipboardMode::Both)
    }

    #[must_use]
    pub fn allows_read(self) -> bool {
        matches!(self, ClipboardMode::FromRemote | ClipboardMode::Both)
    }

    fn name(self) -> &'static str {
        match self {
            ClipboardMode::Off => "off",
            ClipboardMode::ToRemote => "to-remote",
            ClipboardMode::FromRemote => "from-remote",
            ClipboardMode::Both => "both",
        }
    }

    fn parse(text: &str) -> Result<Self, StateError> {
        match text {
            "off" => Ok(ClipboardMode::Off),
            "to-remote" => Ok(ClipboardMode::ToRemote),
            "from-remote" => Ok(ClipboardMode::FromRemote),
            "both" => Ok(ClipboardMode::Both),
            _ => Err(invalid("bad clipboard mode")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SecretStorage {
    #[default]
    File,
    System,
}

impl SecretStorage {
    fn name(self) -> &'static str {
        match self {
            SecretStorage::File => "file",
            SecretStorage::System => "system",
        }
    }

    fn parse(text: &str) -> Result<Self, StateError> {
        match text {
            "file" => Ok(SecretStorage::File),
            "system" => Ok(SecretStorage::System),
            _ => Err(invalid("bad secret storage")),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Secrets {
    pub agent_private: Vec<u8>,
    pub totp_secret: Vec<u8>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secrets(..)")
    }
}

impl Secrets {
    #[must_use]
    pub fn names(agent: &PublicKey) -> (String, String) {
        let identity = agent.to_text();
        (
            format!("{identity}/agent_private"),
            format!("{identity}/totp_secret"),
        )
    }
}

#[derive(Debug, Clone)]
pub struct Device {
    pub key: PublicKey,
    pub name: String,
    pub added_at: u64,
    pub failures: FailureTracker,
}

#[derive(Debug)]
pub struct State {
    storage: SecretStorage,
    agent: StaticKey,
    totp: TotpSecret,
    totp_last_step: Option<u64>,
    locked: bool,
    terminal_allowed: bool,
    clipboard: ClipboardMode,
    file_roots: Vec<PathBuf>,
    pairing: Option<PairingGrant>,
    local: FailureTracker,
    devices: Vec<Device>,
}

impl State {
    pub fn create() -> Result<Self, StateError> {
        Ok(Self {
            storage: SecretStorage::File,
            agent: StaticKey::generate()?,
            totp: TotpSecret::generate()?,
            totp_last_step: None,
            locked: false,
            terminal_allowed: false,
            clipboard: ClipboardMode::Off,
            file_roots: Vec::new(),
            pairing: None,
            local: FailureTracker::default(),
            devices: Vec::new(),
        })
    }

    #[must_use]
    pub fn storage(&self) -> SecretStorage {
        self.storage
    }

    pub fn set_storage(&mut self, storage: SecretStorage) {
        self.storage = storage;
    }

    #[must_use]
    pub fn secrets(&self) -> Secrets {
        Secrets {
            agent_private: self.agent.private_bytes().to_vec(),
            totp_secret: self.totp.to_bytes().to_vec(),
        }
    }

    #[must_use]
    pub fn agent_key(&self) -> &StaticKey {
        &self.agent
    }

    #[must_use]
    pub fn totp(&self) -> &TotpSecret {
        &self.totp
    }

    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    #[must_use]
    pub fn terminal_allowed(&self) -> bool {
        self.terminal_allowed
    }

    pub fn set_terminal_allowed(&mut self, allowed: bool) {
        self.terminal_allowed = allowed;
    }

    #[must_use]
    pub fn clipboard_mode(&self) -> ClipboardMode {
        self.clipboard
    }

    pub fn set_clipboard_mode(&mut self, mode: ClipboardMode) {
        self.clipboard = mode;
    }

    #[must_use]
    pub fn file_roots(&self) -> &[PathBuf] {
        &self.file_roots
    }

    pub fn add_file_root(&mut self, path: &Path) -> Result<bool, StateError> {
        valid_root(path)?;
        if self.file_roots.iter().any(|root| root == path) {
            return Ok(false);
        }
        if self.file_roots.len() >= MAX_FILE_ROOTS {
            return Err(StateError::TooManyRoots);
        }
        self.file_roots.push(path.to_path_buf());
        Ok(true)
    }

    pub fn remove_file_root(&mut self, path: &Path) -> Result<(), StateError> {
        let before = self.file_roots.len();
        self.file_roots.retain(|root| root != path);
        if self.file_roots.len() == before {
            return Err(StateError::NoSuchRoot);
        }
        Ok(())
    }

    pub fn open_pairing(&mut self, now: u64) -> Result<PairingCode, StateError> {
        let code = PairingCode::generate()?;
        self.pairing = Some(PairingGrant::new(&code, now));
        Ok(code)
    }

    #[must_use]
    pub fn pairing_psk(&self, now: u64) -> Option<[u8; PSK_BYTES]> {
        self.pairing
            .as_ref()
            .filter(|grant| grant.usable(now))
            .map(PairingGrant::psk)
    }

    #[must_use]
    pub fn pairing_expires_at(&self) -> Option<u64> {
        self.pairing.as_ref().map(PairingGrant::expires_at)
    }

    pub fn record_pairing_failure(&mut self) {
        if let Some(grant) = self.pairing.as_mut() {
            grant.record_failure();
        }
    }

    pub fn close_pairing(&mut self) {
        self.pairing = None;
    }

    #[must_use]
    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    #[must_use]
    pub fn device(&self, key: &PublicKey) -> Option<&Device> {
        self.devices.iter().find(|device| device.key == *key)
    }

    pub fn add_device(&mut self, key: PublicKey, name: &str, now: u64) -> Result<(), StateError> {
        if self.device(&key).is_some() {
            return Err(StateError::DuplicateDevice);
        }
        if self.devices.len() >= MAX_DEVICES {
            return Err(StateError::TooManyDevices);
        }
        self.devices.push(Device {
            key,
            name: validate_device_name(name)?,
            added_at: now,
            failures: FailureTracker::default(),
        });
        Ok(())
    }

    pub fn find_by_prefix(&self, prefix: &str) -> Result<PublicKey, StateError> {
        let wanted = prefix.trim().to_ascii_uppercase();
        if wanted.is_empty() {
            return Err(StateError::NoSuchDevice);
        }
        let mut matches = self
            .devices
            .iter()
            .filter(|device| device.key.to_text().starts_with(&wanted));
        let first = matches.next().ok_or(StateError::NoSuchDevice)?;
        if matches.next().is_some() {
            return Err(StateError::AmbiguousDevice);
        }
        Ok(first.key)
    }

    pub fn remove_device(&mut self, key: &PublicKey) -> Result<(), StateError> {
        let before = self.devices.len();
        self.devices.retain(|device| device.key != *key);
        if self.devices.len() == before {
            return Err(StateError::NoSuchDevice);
        }
        Ok(())
    }

    pub fn reactivate_device(&mut self, key: &PublicKey) -> Result<(), StateError> {
        let device = self
            .devices
            .iter_mut()
            .find(|device| device.key == *key)
            .ok_or(StateError::NoSuchDevice)?;
        device.failures.reactivate();
        Ok(())
    }

    pub fn check_device(&self, key: &PublicKey, now: u64) -> Result<(), Denied> {
        self.device(key)
            .map_or(Ok(()), |device| device.failures.check(now))
    }

    pub fn verify_totp(
        &mut self,
        device: Option<&PublicKey>,
        code: &str,
        now: u64,
    ) -> Result<(), AuthError> {
        if let Some(known) = device.and_then(|key| self.device(key)) {
            known.failures.check(now).map_err(AuthError::Denied)?;
        }
        if let Some(step) = self.totp.verify(code, now, self.totp_last_step) {
            self.totp_last_step = Some(step);
            if let Some(known) = self.device_mut(device) {
                known.failures.record_success();
            }
            return Ok(());
        }
        if let Some(known) = self.device_mut(device) {
            known.failures.record_failure(now);
        }
        Err(AuthError::WrongCode)
    }

    pub fn verify_local(&mut self, code: &str, now: u64) -> Result<(), AuthError> {
        if let Err(denied) = self.local.check(now) {
            return Err(AuthError::Denied(denied));
        }
        if let Some(step) = self.totp.verify(code, now, self.totp_last_step) {
            self.totp_last_step = Some(step);
            self.local.record_success();
            return Ok(());
        }
        self.local.record_failure(now);
        if self.local.suspended() {
            self.local = FailureTracker::from_parts(
                SUSPEND_AT - 1,
                now.saturating_add(MAX_LOCK_SECONDS),
                false,
            );
        }
        Err(AuthError::WrongCode)
    }

    fn device_mut(&mut self, key: Option<&PublicKey>) -> Option<&mut Device> {
        let key = key?;
        self.devices.iter_mut().find(|device| device.key == *key)
    }

    #[must_use]
    pub fn serialize(&self) -> String {
        let mut out = match self.storage {
            SecretStorage::File => format!(
                "{HEADER}\nagent_private {}\nagent_public {}\ntotp_secret {}\n",
                base32::encode(self.agent.private_bytes()),
                self.agent.public().to_text(),
                self.totp.to_base32()
            ),
            SecretStorage::System => format!(
                "{HEADER}\nagent_public {}\nsecrets {}\n",
                self.agent.public().to_text(),
                self.storage.name()
            ),
        };
        let step = self
            .totp_last_step
            .map_or_else(|| "-".to_owned(), |step| step.to_string());
        let _ = writeln!(out, "totp_last_step {step}");
        let _ = writeln!(out, "locked {}", u8::from(self.locked));
        let _ = writeln!(out, "terminal {}", u8::from(self.terminal_allowed));
        let _ = writeln!(out, "clipboard {}", self.clipboard.name());
        for root in &self.file_roots {
            let _ = writeln!(out, "root {}", root.display());
        }
        if let Some(grant) = &self.pairing {
            let _ = writeln!(
                out,
                "pairing {} {} {}",
                base32::encode(&grant.psk()),
                grant.expires_at(),
                grant.failures()
            );
        }
        let _ = writeln!(
            out,
            "local {} {}",
            self.local.consecutive(),
            self.local.locked_until()
        );
        for device in &self.devices {
            let _ = writeln!(
                out,
                "device {} {} {} {} {} {}",
                device.key.to_text(),
                device.added_at,
                device.failures.consecutive(),
                device.failures.locked_until(),
                u8::from(device.failures.suspended()),
                device.name
            );
        }
        out
    }

    pub fn parse(text: &str) -> Result<Self, StateError> {
        Self::parse_with(text, None)
    }

    pub fn parse_with(text: &str, secrets: Option<&Secrets>) -> Result<Self, StateError> {
        let mut parser = Parser::default();
        for line in body(text)? {
            parser.line(line)?;
        }
        parser.finish(secrets)
    }

    pub fn peek(text: &str) -> Result<(SecretStorage, PublicKey), StateError> {
        let mut storage = SecretStorage::File;
        let mut public = None;
        for line in body(text)? {
            match line.split_once(' ') {
                Some(("secrets", rest)) => storage = SecretStorage::parse(rest)?,
                Some(("agent_public", rest)) => public = Some(PublicKey::from_text(rest)?),
                _ => {}
            }
        }
        let public = public.ok_or_else(|| invalid("missing entry agent_public"))?;
        Ok((storage, public))
    }
}

fn body(text: &str) -> Result<std::str::Lines<'_>, StateError> {
    if text.len() > MAX_STATE_BYTES {
        return Err(StateError::TooLarge);
    }
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(invalid("missing or unknown header"));
    }
    Ok(lines)
}

#[derive(Default)]
struct Parser {
    storage: Option<SecretStorage>,
    private: Option<Vec<u8>>,
    public: Option<Vec<u8>>,
    totp: Option<TotpSecret>,
    last_step: Option<u64>,
    last_step_seen: bool,
    locked: Option<bool>,
    terminal: Option<bool>,
    clipboard: Option<ClipboardMode>,
    file_roots: Vec<PathBuf>,
    pairing: Option<PairingGrant>,
    local: Option<FailureTracker>,
    devices: Vec<Device>,
}

impl Parser {
    fn line(&mut self, line: &str) -> Result<(), StateError> {
        if line.trim().is_empty() {
            return Ok(());
        }
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "secrets" => once(&mut self.storage, SecretStorage::parse(rest)?, key),
            "agent_private" => once(&mut self.private, base32::decode(rest)?, key),
            "agent_public" => once(&mut self.public, base32::decode(rest)?, key),
            "totp_secret" => once(
                &mut self.totp,
                TotpSecret::from_bytes(&base32::decode(rest)?)?,
                key,
            ),
            "totp_last_step" => {
                if self.last_step_seen {
                    return Err(invalid("duplicate entry totp_last_step"));
                }
                self.last_step_seen = true;
                self.last_step = parse_step(rest)?;
                Ok(())
            }
            "locked" => once(&mut self.locked, parse_flag(rest)?, key),
            "terminal" => once(&mut self.terminal, parse_flag(rest)?, key),
            "clipboard" => once(&mut self.clipboard, ClipboardMode::parse(rest)?, key),
            "root" => {
                if self.file_roots.len() >= MAX_FILE_ROOTS {
                    return Err(StateError::TooManyRoots);
                }
                let path = PathBuf::from(rest);
                valid_root(&path)?;
                if self.file_roots.contains(&path) {
                    return Err(invalid("duplicate entry root"));
                }
                self.file_roots.push(path);
                Ok(())
            }
            "pairing" => once(&mut self.pairing, parse_pairing(rest)?, key),
            "local" => once(&mut self.local, parse_local(rest)?, key),
            "device" => {
                if self.devices.len() >= MAX_DEVICES {
                    return Err(StateError::TooManyDevices);
                }
                let device = parse_device(rest)?;
                if self.devices.iter().any(|known| known.key == device.key) {
                    return Err(StateError::DuplicateDevice);
                }
                self.devices.push(device);
                Ok(())
            }
            other => Err(invalid(&format!("unknown entry {other:?}"))),
        }
    }

    fn finish(self, secrets: Option<&Secrets>) -> Result<State, StateError> {
        let missing = |name: &str| invalid(&format!("missing entry {name}"));
        let storage = self.storage.unwrap_or_default();
        let (private, totp) = match storage {
            SecretStorage::File => (
                self.private.ok_or_else(|| missing("agent_private"))?,
                self.totp.ok_or_else(|| missing("totp_secret"))?,
            ),
            SecretStorage::System => {
                if self.private.is_some() || self.totp.is_some() {
                    return Err(invalid(
                        "secrets in the file while the system vault is in use",
                    ));
                }
                let secrets = secrets.ok_or(StateError::SecretsMissing)?;
                (
                    secrets.agent_private.clone(),
                    TotpSecret::from_bytes(&secrets.totp_secret)?,
                )
            }
        };
        let agent = StaticKey::from_parts(
            &private,
            &self.public.ok_or_else(|| missing("agent_public"))?,
        )?;
        Ok(State {
            storage,
            agent,
            totp,
            totp_last_step: if self.last_step_seen {
                self.last_step
            } else {
                return Err(missing("totp_last_step"));
            },
            locked: self.locked.ok_or_else(|| missing("locked"))?,
            terminal_allowed: self.terminal.unwrap_or(false),
            clipboard: self.clipboard.unwrap_or_default(),
            file_roots: self.file_roots,
            pairing: self.pairing,
            local: self.local.unwrap_or_default(),
            devices: self.devices,
        })
    }
}

fn valid_root(path: &Path) -> Result<(), StateError> {
    let text = path.to_string_lossy();
    if !path.is_absolute()
        || path.parent().is_none()
        || text.contains(['\n', '\r', '\0'])
        || text.len() > wirehaus_protocol::MAX_PATH_LEN
    {
        return Err(invalid("bad shared folder"));
    }
    Ok(())
}

fn once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), StateError> {
    if slot.is_some() {
        return Err(invalid(&format!("duplicate entry {name}")));
    }
    *slot = Some(value);
    Ok(())
}

fn parse_step(text: &str) -> Result<Option<u64>, StateError> {
    if text == "-" {
        return Ok(None);
    }
    text.parse().map(Some).map_err(|_| invalid("bad time step"))
}

fn parse_flag(text: &str) -> Result<bool, StateError> {
    match text {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(invalid("bad flag")),
    }
}

fn parse_local(text: &str) -> Result<FailureTracker, StateError> {
    let fields: Vec<&str> = text.split(' ').collect();
    let [consecutive, locked_until] = fields[..] else {
        return Err(invalid("bad local entry"));
    };
    Ok(FailureTracker::from_parts(
        consecutive
            .parse()
            .map_err(|_| invalid("bad local failures"))?,
        locked_until
            .parse()
            .map_err(|_| invalid("bad local lock"))?,
        false,
    ))
}

fn parse_pairing(text: &str) -> Result<PairingGrant, StateError> {
    let fields: Vec<&str> = text.split(' ').collect();
    let [psk, expires, failures] = fields[..] else {
        return Err(invalid("bad pairing entry"));
    };
    let psk: [u8; PSK_BYTES] = base32::decode(psk)?
        .try_into()
        .map_err(|_| invalid("bad pairing key"))?;
    Ok(PairingGrant::from_parts(
        psk,
        expires.parse().map_err(|_| invalid("bad pairing expiry"))?,
        failures
            .parse()
            .map_err(|_| invalid("bad pairing failures"))?,
    ))
}

fn parse_device(text: &str) -> Result<Device, StateError> {
    let fields: Vec<&str> = text.splitn(6, ' ').collect();
    let [key, added, consecutive, locked_until, suspended, name] = fields[..] else {
        return Err(invalid("bad device entry"));
    };
    let number = |value: &str| {
        value
            .parse::<u64>()
            .map_err(|_| invalid("bad device number"))
    };
    Ok(Device {
        key: PublicKey::from_text(key)?,
        name: validate_device_name(name)?,
        added_at: number(added)?,
        failures: FailureTracker::from_parts(
            u32::try_from(number(consecutive)?).map_err(|_| invalid("bad failure count"))?,
            number(locked_until)?,
            parse_flag(suspended)?,
        ),
    })
}

fn invalid(reason: &str) -> StateError {
    StateError::Invalid(reason.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirehaus_crypto::lockout::SUSPEND_AT;

    fn key() -> PublicKey {
        StaticKey::generate().unwrap().public()
    }

    fn state_with_device() -> (State, PublicKey) {
        let mut state = State::create().unwrap();
        let device = key();
        state.add_device(device, "Aula 3", 1000).unwrap();
        (state, device)
    }

    fn current_code(state: &State, now: u64) -> String {
        state.totp().code_at_step(TotpSecret::step_at(now))
    }

    #[test]
    fn a_new_state_is_unlocked_with_no_devices_and_no_pairing() {
        let state = State::create().unwrap();
        assert!(!state.is_locked());
        assert!(state.devices().is_empty());
        assert_eq!(state.pairing_psk(0), None);
    }

    #[test]
    fn the_whole_state_survives_a_round_trip() {
        let (mut state, device) = state_with_device();
        let second = key();
        state.add_device(second, "Portátil de clase", 2000).unwrap();
        state.open_pairing(5000).unwrap();
        state.set_locked(true);
        let _ = state.verify_totp(Some(&device), "000000", 5000);
        let _ = state.verify_totp(Some(&device), "000001", 5000);
        let _ = state.verify_totp(Some(&device), "000002", 5000);
        let text = state.serialize();
        let restored = State::parse(&text).unwrap();
        assert_eq!(restored.serialize(), text);
        assert_eq!(restored.agent_key().public(), state.agent_key().public());
        assert_eq!(restored.totp().to_bytes(), state.totp().to_bytes());
        assert!(restored.is_locked());
        assert_eq!(restored.devices().len(), 2);
        assert_eq!(restored.device(&second).unwrap().name, "Portátil de clase");
        assert_eq!(restored.pairing_psk(5001), state.pairing_psk(5001));
        assert!(restored.check_device(&device, 5000).is_err());
    }

    #[test]
    fn a_state_that_keeps_its_secrets_in_the_vault_has_none_of_them_in_its_text() {
        let (mut state, device) = state_with_device();
        let secrets = state.secrets();
        state.set_storage(SecretStorage::System);
        let text = state.serialize();
        assert!(!text.contains("agent_private"));
        assert!(!text.contains("totp_secret"));
        assert!(!text.contains(&base32::encode(&secrets.agent_private)));
        assert!(!text.contains(&base32::encode(&secrets.totp_secret)));
        assert!(text.contains("secrets system"));
        let restored = State::parse_with(&text, Some(&secrets)).unwrap();
        assert_eq!(restored.storage(), SecretStorage::System);
        assert_eq!(restored.serialize(), text);
        assert_eq!(restored.secrets(), secrets);
        assert!(restored.device(&device).is_some());
    }

    #[test]
    fn a_state_in_the_vault_cannot_be_read_without_the_secrets_or_with_copies_in_the_file() {
        let (mut state, _) = state_with_device();
        let secrets = state.secrets();
        state.set_storage(SecretStorage::System);
        let text = state.serialize();
        assert_eq!(State::parse(&text).unwrap_err(), StateError::SecretsMissing);
        let (_, public) = State::peek(&text).unwrap();
        assert_eq!(public, state.agent_key().public());
        let leaked = text.replace(
            "secrets system\n",
            &format!(
                "secrets system\nagent_private {}\n",
                base32::encode(&secrets.agent_private)
            ),
        );
        assert!(State::parse_with(&leaked, Some(&secrets)).is_err());
    }

    #[test]
    fn a_state_without_the_storage_line_is_a_file_state_and_the_line_is_checked() {
        let state = State::create().unwrap();
        let text = state.serialize();
        assert!(!text.contains("secrets"));
        assert_eq!(State::peek(&text).unwrap().0, SecretStorage::File);
        assert_eq!(State::parse(&text).unwrap().storage(), SecretStorage::File);
        let bad = format!("{text}secrets maybe\n");
        assert!(State::parse(&bad).is_err());
        let twice = format!("{text}secrets file\nsecrets file\n");
        assert!(State::parse(&twice).is_err());
    }

    #[test]
    fn the_secrets_do_not_show_in_debug_output() {
        let state = State::create().unwrap();
        assert_eq!(format!("{:?}", state.secrets()), "Secrets(..)");
    }

    #[test]
    fn the_clipboard_permission_is_off_by_default_and_persists_per_direction() {
        let mut state = State::create().unwrap();
        assert_eq!(state.clipboard_mode(), ClipboardMode::Off);
        assert!(!state.clipboard_mode().allows_read());
        assert!(!state.clipboard_mode().allows_write());
        for (mode, read, write) in [
            (ClipboardMode::ToRemote, false, true),
            (ClipboardMode::FromRemote, true, false),
            (ClipboardMode::Both, true, true),
            (ClipboardMode::Off, false, false),
        ] {
            state.set_clipboard_mode(mode);
            let restored = State::parse(&state.serialize()).unwrap().clipboard_mode();
            assert_eq!(restored, mode);
            assert_eq!(restored.allows_read(), read);
            assert_eq!(restored.allows_write(), write);
        }
        let older = state.serialize().replace("clipboard off\n", "");
        assert_eq!(
            State::parse(&older).unwrap().clipboard_mode(),
            ClipboardMode::Off
        );
        let broken = state
            .serialize()
            .replace("clipboard off", "clipboard maybe");
        assert!(State::parse(&broken).is_err());
        let twice = format!("{}clipboard both\n", state.serialize());
        assert!(State::parse(&twice).is_err());
    }

    #[test]
    fn the_terminal_permission_is_off_by_default_and_persists() {
        let mut state = State::create().unwrap();
        assert!(!state.terminal_allowed());
        state.set_terminal_allowed(true);
        assert!(State::parse(&state.serialize()).unwrap().terminal_allowed());
        state.set_terminal_allowed(false);
        assert!(!State::parse(&state.serialize()).unwrap().terminal_allowed());
        let older = state.serialize().replace("terminal 0\n", "");
        assert!(!State::parse(&older).unwrap().terminal_allowed());
    }

    #[test]
    fn parsing_rejects_a_bad_header_and_unknown_entries() {
        let text = State::create().unwrap().serialize();
        assert!(State::parse("").is_err());
        assert!(State::parse(&text.replace(HEADER, "wirehaus-state 2")).is_err());
        assert!(State::parse(&format!("{text}surprise 1\n")).is_err());
    }

    #[test]
    fn parsing_rejects_missing_and_duplicated_entries() {
        let text = State::create().unwrap().serialize();
        let kept: Vec<&str> = text
            .lines()
            .filter(|line| !line.starts_with("totp_secret"))
            .collect();
        let without_secret = kept.join("\n") + "\n";
        assert!(State::parse(&without_secret).is_err());
        let duplicated = format!("{text}locked 0\n");
        assert!(State::parse(&duplicated).is_err());
    }

    #[test]
    fn parsing_rejects_malformed_values() {
        let text = State::create().unwrap().serialize();
        for (from, to) in [
            ("locked 0", "locked 2"),
            ("totp_last_step -", "totp_last_step abc"),
            ("totp_last_step -", "totp_last_step -1"),
        ] {
            assert!(State::parse(&text.replace(from, to)).is_err(), "{to}");
        }
        let (state, _) = state_with_device();
        let text = state.serialize();
        assert!(State::parse(&text.replace(" 1000 0 0 0 Aula 3", " x 0 0 0 Aula 3")).is_err());
        assert!(State::parse(&text.replace(" 1000 0 0 0 Aula 3", " 1000 0 0 0 ")).is_err());
        assert!(State::parse(&text.replace(" 1000 0 0 0 Aula 3", " 1000 0 0 7 Aula 3")).is_err());
    }

    #[test]
    fn parsing_rejects_oversized_input() {
        assert!(matches!(
            State::parse(&"a".repeat(MAX_STATE_BYTES + 1)),
            Err(StateError::TooLarge)
        ));
    }

    #[test]
    fn a_mismatched_device_list_cannot_exceed_the_limit() {
        let mut state = State::create().unwrap();
        for index in 0..MAX_DEVICES {
            state.add_device(key(), &format!("dev {index}"), 1).unwrap();
        }
        assert_eq!(
            state.add_device(key(), "one too many", 1),
            Err(StateError::TooManyDevices)
        );
        let mut text = state.serialize();
        writeln!(text, "device {} 1 0 0 0 extra", key().to_text()).unwrap();
        assert_eq!(State::parse(&text).unwrap_err(), StateError::TooManyDevices);
    }

    #[test]
    fn duplicate_devices_are_refused_in_memory_and_in_the_file() {
        let (mut state, device) = state_with_device();
        assert_eq!(
            state.add_device(device, "again", 1),
            Err(StateError::DuplicateDevice)
        );
        let text = format!(
            "{}device {} 1 0 0 0 clone\n",
            state.serialize(),
            device.to_text()
        );
        assert_eq!(
            State::parse(&text).unwrap_err(),
            StateError::DuplicateDevice
        );
    }

    #[test]
    fn a_device_name_is_validated_and_trimmed() {
        let mut state = State::create().unwrap();
        assert!(state.add_device(key(), "bad\nname", 1).is_err());
        assert!(state.add_device(key(), "", 1).is_err());
        let device = key();
        state.add_device(device, "  Casa  ", 1).unwrap();
        assert_eq!(state.device(&device).unwrap().name, "Casa");
    }

    #[test]
    fn pairing_is_usable_until_it_expires_and_can_be_closed() {
        let mut state = State::create().unwrap();
        assert!(state.pairing_psk(0).is_none());
        let code = state.open_pairing(1000).unwrap();
        assert_eq!(state.pairing_psk(1001), Some(code.psk()));
        assert!(state.pairing_psk(1000 + 600).is_none());
        state.close_pairing();
        assert!(state.pairing_psk(1001).is_none());
    }

    #[test]
    fn a_new_pairing_code_replaces_the_old_one() {
        let mut state = State::create().unwrap();
        let first = state.open_pairing(0).unwrap();
        let second = state.open_pairing(0).unwrap();
        assert_ne!(first.psk(), second.psk());
        assert_eq!(state.pairing_psk(1), Some(second.psk()));
    }

    #[test]
    fn too_many_pairing_failures_burn_the_code() {
        let mut state = State::create().unwrap();
        state.open_pairing(0).unwrap();
        for _ in 0..4 {
            state.record_pairing_failure();
            assert!(state.pairing_psk(1).is_some());
        }
        state.record_pairing_failure();
        assert!(state.pairing_psk(1).is_none());
    }

    #[test]
    fn a_correct_code_authenticates_and_a_step_cannot_be_reused() {
        let (mut state, device) = state_with_device();
        let now = 1_000_000;
        let code = current_code(&state, now);
        assert_eq!(state.verify_totp(Some(&device), &code, now), Ok(()));
        assert_eq!(
            state.verify_totp(Some(&device), &code, now),
            Err(AuthError::WrongCode)
        );
        let next = current_code(&state, now + 30);
        assert_eq!(state.verify_totp(Some(&device), &next, now + 30), Ok(()));
    }

    #[test]
    fn local_codes_are_rate_limited_and_never_suspend_for_good() {
        let mut state = State::create().unwrap();
        let mut now = 10_000;
        for _ in 0..30 {
            now += 4000;
            let _ = state.verify_local("000000", now);
        }
        let right = current_code(&state, now + 1);
        assert!(matches!(
            state.verify_local(&right, now + 1),
            Err(AuthError::Denied(Denied::Locked { .. }))
        ));
        let later = now + 1 + MAX_LOCK_SECONDS + 31;
        let right = current_code(&state, later);
        assert_eq!(state.verify_local(&right, later), Ok(()));
    }

    #[test]
    fn a_local_code_cannot_be_reused_and_shares_the_step_with_remote_logins() {
        let (mut state, device) = state_with_device();
        let now = 77_000;
        let code = current_code(&state, now);
        assert_eq!(state.verify_totp(Some(&device), &code, now), Ok(()));
        assert_eq!(state.verify_local(&code, now), Err(AuthError::WrongCode));
    }

    #[test]
    fn the_local_counter_survives_a_round_trip() {
        let mut state = State::create().unwrap();
        for _ in 0..4 {
            let _ = state.verify_local("000000", 500);
        }
        let restored = State::parse(&state.serialize()).unwrap();
        assert_eq!(restored.local.consecutive(), 3);
        assert!(restored.local.check(501).is_err());
        assert!(restored.local.check(531).is_ok());
    }

    #[test]
    fn a_wrong_code_counts_against_the_device_and_locks_it() {
        let (mut state, device) = state_with_device();
        let now = 5000;
        for _ in 0..3 {
            assert_eq!(
                state.verify_totp(Some(&device), "000000", now),
                Err(AuthError::WrongCode)
            );
        }
        let right = current_code(&state, now);
        assert!(matches!(
            state.verify_totp(Some(&device), &right, now),
            Err(AuthError::Denied(Denied::Locked { .. }))
        ));
        assert_eq!(state.verify_totp(Some(&device), &right, now + 31), Ok(()));
    }

    #[test]
    fn the_tenth_consecutive_failure_suspends_the_device_until_the_owner_reactivates_it() {
        let (mut state, device) = state_with_device();
        let mut now = 5000;
        for _ in 0..SUSPEND_AT {
            now += 4000;
            let _ = state.verify_totp(Some(&device), "000000", now);
        }
        let right = current_code(&state, now + 4000);
        assert_eq!(
            state.verify_totp(Some(&device), &right, now + 4000),
            Err(AuthError::Denied(Denied::Suspended))
        );
        state.reactivate_device(&device).unwrap();
        assert_eq!(state.verify_totp(Some(&device), &right, now + 4000), Ok(()));
    }

    #[test]
    fn a_lockout_on_one_device_does_not_affect_another() {
        let (mut state, first) = state_with_device();
        let second = key();
        state.add_device(second, "Otro", 1).unwrap();
        for _ in 0..4 {
            let _ = state.verify_totp(Some(&first), "000000", 5000);
        }
        assert!(state.check_device(&first, 5000).is_err());
        assert!(state.check_device(&second, 5000).is_ok());
        let code = current_code(&state, 5000);
        assert_eq!(state.verify_totp(Some(&second), &code, 5000), Ok(()));
    }

    #[test]
    fn a_pairing_candidate_has_no_device_tracker() {
        let mut state = State::create().unwrap();
        let code = current_code(&state, 9000);
        assert_eq!(state.verify_totp(None, &code, 9000), Ok(()));
        assert_eq!(
            state.verify_totp(None, &code, 9000),
            Err(AuthError::WrongCode)
        );
    }

    #[test]
    fn a_success_resets_the_failure_counter() {
        let (mut state, device) = state_with_device();
        let _ = state.verify_totp(Some(&device), "000000", 5000);
        let _ = state.verify_totp(Some(&device), "000000", 5000);
        let code = current_code(&state, 5000);
        state.verify_totp(Some(&device), &code, 5000).unwrap();
        assert_eq!(state.device(&device).unwrap().failures.consecutive(), 0);
    }

    #[test]
    fn devices_are_found_by_a_unique_key_prefix() {
        let (mut state, device) = state_with_device();
        let other = key();
        state.add_device(other, "Otro", 1).unwrap();
        let text = device.to_text();
        assert_eq!(state.find_by_prefix(&text[..12]).unwrap(), device);
        assert_eq!(state.find_by_prefix(&text.to_lowercase()).unwrap(), device);
        assert_eq!(state.find_by_prefix(""), Err(StateError::NoSuchDevice));
        assert_eq!(
            state.find_by_prefix("ZZZZZZZZZZ"),
            Err(StateError::NoSuchDevice)
        );
        let common = (0..text.len())
            .find(|&n| !other.to_text().starts_with(&text[..=n]))
            .unwrap_or(text.len());
        if common > 0 {
            assert_eq!(
                state.find_by_prefix(&text[..common]),
                Err(StateError::AmbiguousDevice)
            );
        }
    }

    #[test]
    fn removing_a_device_revokes_it() {
        let (mut state, device) = state_with_device();
        state.remove_device(&device).unwrap();
        assert!(state.device(&device).is_none());
        assert_eq!(state.remove_device(&device), Err(StateError::NoSuchDevice));
        assert_eq!(
            state.reactivate_device(&device),
            Err(StateError::NoSuchDevice)
        );
    }

    #[test]
    fn secrets_do_not_leak_through_debug() {
        let state = State::create().unwrap();
        let text = format!("{state:?}");
        assert!(!text.contains(&base32::encode(state.agent_key().private_bytes())));
        assert!(!text.contains(&state.totp().to_base32()));
    }

    #[test]
    fn parsing_never_panics_and_what_parses_serializes_stably() {
        let mut rng = crate::testing::Rng::new(0xFEED_FACE_CAFE_BEEF);
        let (mut state, _device) = state_with_device();
        state.open_pairing(100).unwrap();
        state.set_locked(true);
        let valid = state.serialize().into_bytes();
        let mut accepted = 0;
        for round in 0..20_000 {
            let input = if round % 4 == 0 {
                rng.bytes(300)
            } else {
                rng.mutate(&valid)
            };
            let text = String::from_utf8_lossy(&input).into_owned();
            if let Ok(parsed) = State::parse(&text) {
                accepted += 1;
                let again = State::parse(&parsed.serialize()).unwrap();
                assert_eq!(again.serialize(), parsed.serialize());
                assert!(parsed.devices().len() <= MAX_DEVICES);
            }
        }
        assert!(accepted > 50);
    }
}
