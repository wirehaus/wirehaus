use std::io::Write;
use std::path::Path;

use thiserror::Error;
use wirehaus_crypto::lockout::Denied;
use wirehaus_crypto::pairing::VALIDITY_SECONDS;

use crate::state::{AuthError, State, StateError};
use crate::store::{Clock, Store, StoreError};

const ISSUER: &str = "Wirehaus";

#[derive(Debug, Error)]
pub enum CommandError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error("no se puede abrir la carpeta: {0}")]
    Folder(std::io::Error),
    #[error("{0} no es una carpeta")]
    NotAFolder(std::path::PathBuf),
    #[error("el codigo es incorrecto")]
    WrongCode,
    #[error("demasiados intentos: espera {0} segundos")]
    TooManyAttempts(u64),
    #[error("no se puede escribir la salida: {0}")]
    Output(#[from] std::io::Error),
}

pub fn init(dir: &Path, clock: Clock, out: &mut impl Write) -> Result<(), CommandError> {
    let store = Store::initialize(dir, clock)?;
    let (public, secret, uri) = store.read(|state, _| {
        (
            state.agent_key().public().to_text(),
            state.totp().to_base32(),
            state.totp().otpauth_uri(ISSUER, &host_name()),
        )
    })?;
    store.log("init", "agent identity created");
    writeln!(out, "Identidad del agente creada en {}.", dir.display())?;
    writeln!(out, "Clave publica del agente: {public}")?;
    writeln!(out)?;
    writeln!(
        out,
        "Segundo factor: anade este secreto a tu aplicacion de autenticacion. Solo se muestra ahora."
    )?;
    writeln!(out, "Secreto: {secret}")?;
    writeln!(out, "URI: {uri}")?;
    Ok(())
}

pub fn pairing(store: &Store, out: &mut impl Write) -> Result<(), CommandError> {
    let code = store.update(State::open_pairing)??;
    store.log("pairing_opened", "a pairing code was generated");
    writeln!(out, "Codigo de emparejamiento: {}", code.to_text())?;
    writeln!(
        out,
        "Vale una sola vez y caduca en {} minutos. Para entrar tambien hara falta el codigo del segundo factor.",
        VALIDITY_SECONDS / 60
    )?;
    Ok(())
}

pub fn devices(store: &Store, out: &mut impl Write) -> Result<(), CommandError> {
    let lines = store.read(|state, now| {
        state
            .devices()
            .iter()
            .map(|device| {
                let status = match device.failures.check(now) {
                    Ok(()) => "activo".to_owned(),
                    Err(Denied::Suspended) => "suspendido".to_owned(),
                    Err(Denied::Locked { retry_after }) => format!("bloqueado {retry_after} s"),
                };
                format!("{} {} ({status})", device.key.to_text(), device.name)
            })
            .collect::<Vec<_>>()
    })?;
    if lines.is_empty() {
        writeln!(out, "No hay dispositivos emparejados.")?;
    }
    for line in lines {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

pub fn revoke(
    store: &Store,
    prefix: &str,
    code: &str,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    verify(store, code)?;
    let name = store.update(|state, _| {
        let key = state.find_by_prefix(prefix)?;
        let name = state.device(&key).map(|device| device.name.clone());
        state.remove_device(&key)?;
        Ok::<_, StateError>(name.unwrap_or_default())
    })??;
    store.log("revoked", &name);
    writeln!(out, "Dispositivo revocado: {name}")?;
    Ok(())
}

pub fn reactivate(
    store: &Store,
    prefix: &str,
    code: &str,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    verify(store, code)?;
    let name = store.update(|state, _| {
        let key = state.find_by_prefix(prefix)?;
        state.reactivate_device(&key)?;
        Ok::<_, StateError>(
            state
                .device(&key)
                .map(|device| device.name.clone())
                .unwrap_or_default(),
        )
    })??;
    store.log("reactivated", &name);
    writeln!(out, "Dispositivo reactivado: {name}")?;
    Ok(())
}

pub fn lock(
    store: &Store,
    locked: bool,
    code: &str,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    verify(store, code)?;
    store.update(|state, _| state.set_locked(locked))?;
    store.log(
        if locked { "locked" } else { "unlocked" },
        "by the local administrator",
    );
    writeln!(
        out,
        "{}",
        if locked {
            "Acceso bloqueado: no se aceptan conexiones."
        } else {
            "Acceso desbloqueado."
        }
    )?;
    Ok(())
}

pub fn files_add(
    store: &Store,
    folder: &Path,
    code: &str,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    verify(store, code)?;
    let real = folder.canonicalize().map_err(CommandError::Folder)?;
    if !real.is_dir() {
        return Err(CommandError::NotAFolder(real));
    }
    let added = store.update(|state, _| state.add_file_root(&real))??;
    if added {
        store.log("files_root_added", &real.display().to_string());
    }
    writeln!(out, "Carpeta compartida: {}", real.display())?;
    Ok(())
}

pub fn files_remove(
    store: &Store,
    folder: &Path,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    let target = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
    store.update(|state, _| state.remove_file_root(&target))??;
    store.log("files_root_removed", &target.display().to_string());
    writeln!(out, "Carpeta retirada: {}", target.display())?;
    Ok(())
}

pub fn files_list(store: &Store, out: &mut impl Write) -> Result<(), CommandError> {
    let roots = store.read(|state, _| state.file_roots().to_vec())?;
    if roots.is_empty() {
        writeln!(out, "No hay carpetas compartidas.")?;
    }
    for root in roots {
        writeln!(out, "{}", root.display())?;
    }
    Ok(())
}

pub fn terminal(
    store: &Store,
    allow: bool,
    code: Option<&str>,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    if allow {
        verify(store, code.unwrap_or_default())?;
    }
    store.update(|state, _| state.set_terminal_allowed(allow))?;
    store.log(
        if allow {
            "terminal_allowed"
        } else {
            "terminal_denied"
        },
        "by the local administrator",
    );
    writeln!(
        out,
        "{}",
        if allow {
            "Terminal remota permitida."
        } else {
            "Terminal remota desactivada."
        }
    )?;
    Ok(())
}

fn verify(store: &Store, code: &str) -> Result<(), CommandError> {
    match store.update(|state, now| state.verify_local(code, now))? {
        Ok(()) => Ok(()),
        Err(AuthError::WrongCode) => {
            store.log("local_code_failed", "wrong code on the command line");
            Err(CommandError::WrongCode)
        }
        Err(AuthError::Denied(Denied::Locked { retry_after })) => {
            Err(CommandError::TooManyAttempts(retry_after))
        }
        Err(AuthError::Denied(Denied::Suspended)) => Err(CommandError::TooManyAttempts(3600)),
    }
}

fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "equipo".to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use wirehaus_crypto::keys::StaticKey;
    use wirehaus_crypto::totp::TotpSecret;

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            Self(
                std::env::temp_dir()
                    .join(format!(
                        "wirehaus-commands-{}-{}",
                        std::process::id(),
                        COUNTER.fetch_add(1, Ordering::SeqCst)
                    ))
                    .join("agent"),
            )
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            if let Some(parent) = self.0.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
    }

    fn clock(time: &Arc<AtomicU64>) -> Clock {
        let shared = Arc::clone(time);
        Arc::new(move || shared.load(Ordering::SeqCst))
    }

    fn setup() -> (TempDir, Store, Arc<AtomicU64>) {
        let dir = TempDir::new();
        let time = Arc::new(AtomicU64::new(1_000_000));
        let mut sink = Vec::new();
        init(&dir.0, clock(&time), &mut sink).unwrap();
        let store = Store::open(&dir.0, clock(&time)).unwrap();
        (dir, store, time)
    }

    fn code(store: &Store, time: &AtomicU64) -> String {
        let now = time.load(Ordering::SeqCst);
        store
            .read(|state, _| state.totp().code_at_step(TotpSecret::step_at(now)))
            .unwrap()
    }

    fn next_step(time: &AtomicU64) {
        time.fetch_add(31, Ordering::SeqCst);
    }

    fn add_device(store: &Store, name: &str) -> String {
        let key = StaticKey::generate().unwrap().public();
        store
            .update(|state, now| state.add_device(key, name, now))
            .unwrap()
            .unwrap();
        key.to_text()
    }

    fn text(out: Vec<u8>) -> String {
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn init_prints_the_public_key_the_secret_and_the_uri_once() {
        let dir = TempDir::new();
        let time = Arc::new(AtomicU64::new(0));
        let mut out = Vec::new();
        init(&dir.0, clock(&time), &mut out).unwrap();
        let shown = text(out);
        let store = Store::open(&dir.0, clock(&time)).unwrap();
        let (public, secret) = store
            .read(|state, _| {
                (
                    state.agent_key().public().to_text(),
                    state.totp().to_base32(),
                )
            })
            .unwrap();
        assert!(shown.contains(&public));
        assert!(shown.contains(&secret));
        assert!(shown.contains("otpauth://totp/Wirehaus:"));
        assert!(matches!(
            init(&dir.0, clock(&time), &mut Vec::new()),
            Err(CommandError::Store(StoreError::AlreadyInitialized(_)))
        ));
    }

    #[test]
    fn pairing_prints_a_usable_code_and_opens_the_grant() {
        let (_dir, store, time) = setup();
        let mut out = Vec::new();
        pairing(&store, &mut out).unwrap();
        let shown = text(out);
        assert!(shown.contains("Codigo de emparejamiento: "));
        assert!(shown.contains("10 minutos"));
        let now = time.load(Ordering::SeqCst);
        assert!(
            store
                .read(|state, _| state.pairing_psk(now))
                .unwrap()
                .is_some()
        );
        let code_text = shown
            .lines()
            .next()
            .unwrap()
            .trim_start_matches("Codigo de emparejamiento: ");
        let parsed = wirehaus_crypto::pairing::PairingCode::from_text(code_text).unwrap();
        assert_eq!(
            store.read(|state, _| state.pairing_psk(now)).unwrap(),
            Some(parsed.psk())
        );
    }

    #[test]
    fn listing_shows_the_status_of_each_device() {
        let (_dir, store, _time) = setup();
        let mut out = Vec::new();
        devices(&store, &mut out).unwrap();
        assert!(text(out).contains("No hay dispositivos"));
        let key = add_device(&store, "Aula 3");
        let mut out = Vec::new();
        devices(&store, &mut out).unwrap();
        let shown = text(out);
        assert!(shown.contains(&key));
        assert!(shown.contains("Aula 3 (activo)"));
    }

    #[test]
    fn revoking_needs_a_valid_code_and_removes_the_device() {
        let (_dir, store, time) = setup();
        let key = add_device(&store, "Aula 3");
        let mut out = Vec::new();
        assert!(matches!(
            revoke(&store, &key[..10], "000000", &mut out),
            Err(CommandError::WrongCode)
        ));
        assert_eq!(store.read(|state, _| state.devices().len()).unwrap(), 1);
        let right = code(&store, &time);
        revoke(&store, &key[..10], &right, &mut out).unwrap();
        assert_eq!(store.read(|state, _| state.devices().len()).unwrap(), 0);
        assert!(text(out).contains("Dispositivo revocado: Aula 3"));
    }

    #[test]
    fn revoking_an_unknown_device_is_an_error() {
        let (_dir, store, time) = setup();
        let right = code(&store, &time);
        assert!(matches!(
            revoke(&store, "ZZZZZZ", &right, &mut Vec::new()),
            Err(CommandError::State(StateError::NoSuchDevice))
        ));
    }

    #[test]
    fn a_code_cannot_be_used_twice_on_the_command_line() {
        let (_dir, store, time) = setup();
        let first = add_device(&store, "Uno");
        let second = add_device(&store, "Dos");
        let right = code(&store, &time);
        revoke(&store, &first[..10], &right, &mut Vec::new()).unwrap();
        assert!(matches!(
            revoke(&store, &second[..10], &right, &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        next_step(&time);
        let next = code(&store, &time);
        revoke(&store, &second[..10], &next, &mut Vec::new()).unwrap();
    }

    #[test]
    fn locking_and_unlocking_need_the_code() {
        let (_dir, store, time) = setup();
        assert!(matches!(
            lock(&store, true, "123456", &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        assert!(!store.read(|state, _| state.is_locked()).unwrap());
        let right = code(&store, &time);
        let mut out = Vec::new();
        lock(&store, true, &right, &mut out).unwrap();
        assert!(store.read(|state, _| state.is_locked()).unwrap());
        assert!(text(out).contains("bloqueado"));
        next_step(&time);
        let next = code(&store, &time);
        lock(&store, false, &next, &mut Vec::new()).unwrap();
        assert!(!store.read(|state, _| state.is_locked()).unwrap());
    }

    #[test]
    fn allowing_the_terminal_needs_a_code_and_denying_it_does_not() {
        let (_dir, store, time) = setup();
        assert!(matches!(
            terminal(&store, true, Some("000000"), &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        assert!(matches!(
            terminal(&store, true, None, &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        assert!(!store.read(|state, _| state.terminal_allowed()).unwrap());
        let right = code(&store, &time);
        terminal(&store, true, Some(&right), &mut Vec::new()).unwrap();
        assert!(store.read(|state, _| state.terminal_allowed()).unwrap());
        terminal(&store, false, None, &mut Vec::new()).unwrap();
        assert!(!store.read(|state, _| state.terminal_allowed()).unwrap());
    }

    #[test]
    fn repeated_wrong_codes_make_the_command_line_wait() {
        let (_dir, store, _time) = setup();
        for _ in 0..3 {
            let _ = lock(&store, true, "000000", &mut Vec::new());
        }
        assert!(matches!(
            lock(&store, true, "000000", &mut Vec::new()),
            Err(CommandError::TooManyAttempts(_))
        ));
    }

    #[test]
    fn reactivating_clears_a_suspension() {
        let (_dir, store, time) = setup();
        let key = add_device(&store, "Aula 3");
        let device = store
            .read(|state, _| state.find_by_prefix(&key[..12]).unwrap())
            .unwrap();
        for index in 0..10 {
            time.fetch_add(4000 * (index + 1), Ordering::SeqCst);
            let _ = store.update(|state, now| state.verify_totp(Some(&device), "000000", now));
        }
        let mut out = Vec::new();
        devices(&store, &mut out).unwrap();
        assert!(text(out).contains("suspendido"));
        next_step(&time);
        let right = code(&store, &time);
        reactivate(&store, &key[..12], &right, &mut Vec::new()).unwrap();
        let mut out = Vec::new();
        devices(&store, &mut out).unwrap();
        assert!(text(out).contains("activo"));
    }

    #[test]
    fn every_administrative_action_is_logged_without_secrets() {
        let (dir, store, time) = setup();
        let key = add_device(&store, "Aula 3");
        pairing(&store, &mut Vec::new()).unwrap();
        let right = code(&store, &time);
        revoke(&store, &key[..10], &right, &mut Vec::new()).unwrap();
        let log = std::fs::read_to_string(dir.0.join("access.log")).unwrap();
        for event in ["init", "pairing_opened", "revoked"] {
            assert!(log.contains(event), "{event}");
        }
        let secret = store.read(|state, _| state.totp().to_base32()).unwrap();
        assert!(!log.contains(&secret));
    }
}
