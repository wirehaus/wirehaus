use std::io::Write;
use std::path::Path;

use thiserror::Error;
use wirehaus_crypto::lockout::Denied;
use wirehaus_crypto::pairing::VALIDITY_SECONDS;

use crate::i18n::tr;
use crate::state::{AuthError, ClipboardMode, State, StateError};
use crate::store::{Clock, Store, StoreError};

const ISSUER: &str = "Wirehaus";

#[derive(Debug, Error)]
pub enum CommandError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error("{}", self.describe())]
    Folder(std::io::Error),
    #[error("{}", self.describe())]
    NotAFolder(std::path::PathBuf),
    #[error("{}", self.describe())]
    WrongCode,
    #[error("{}", self.describe())]
    TooManyAttempts(u64),
    #[error("{}", self.describe())]
    Output(#[from] std::io::Error),
}

impl CommandError {
    fn describe(&self) -> String {
        match self {
            CommandError::Folder(error) => tr!(
                format!("no se puede abrir la carpeta: {error}"),
                format!("cannot open the folder: {error}")
            ),
            CommandError::NotAFolder(path) => tr!(
                format!("{} no es una carpeta", path.display()),
                format!("{} is not a folder", path.display())
            ),
            CommandError::WrongCode => tr!("el código es incorrecto", "the code is wrong"),
            CommandError::TooManyAttempts(seconds) => tr!(
                format!("demasiados intentos: espera {seconds} segundos"),
                format!("too many attempts: wait {seconds} seconds")
            ),
            CommandError::Output(error) => tr!(
                format!("no se puede escribir la salida: {error}"),
                format!("cannot write the output: {error}")
            ),
            CommandError::Store(error) => error.to_string(),
            CommandError::State(error) => error.to_string(),
        }
    }
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
    let location = dir.display();
    writeln!(
        out,
        "{}",
        tr!(
            format!("Identidad del agente creada en {location}."),
            format!("Agent identity created in {location}.")
        )
    )?;
    writeln!(
        out,
        "{}",
        tr!(
            format!("Clave pública del agente: {public}"),
            format!("Agent public key: {public}")
        )
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        tr!(
            "Segundo factor: añade este secreto a tu aplicación de autenticación. Solo se muestra ahora.",
            "Second factor: add this secret to your authenticator app. It is only shown now."
        )
    )?;
    writeln!(out, "{}: {secret}", tr!("Secreto", "Secret"))?;
    writeln!(out, "URI: {uri}")?;
    Ok(())
}

pub fn permissions(
    screen_recording: bool,
    input_control: bool,
    out: &mut impl Write,
) -> Result<bool, CommandError> {
    let state = |granted: bool| {
        if granted {
            tr!("concedido", "granted")
        } else {
            tr!("pendiente", "pending")
        }
    };
    writeln!(
        out,
        "{}: {}",
        tr!("Grabación de pantalla", "Screen recording"),
        state(screen_recording)
    )?;
    writeln!(
        out,
        "{}: {}",
        tr!(
            "Control de teclado y ratón (Accesibilidad)",
            "Keyboard and mouse control (Accessibility)"
        ),
        state(input_control)
    )?;
    let complete = screen_recording && input_control;
    if complete {
        writeln!(
            out,
            "{}",
            tr!(
                "El agente tiene todos los permisos que necesita.",
                "The agent has every permission it needs."
            )
        )?;
    } else {
        writeln!(
            out,
            "{}",
            tr!(
                "Concede lo pendiente en Ajustes del Sistema > Privacidad y seguridad, al programa que lanza el agente (la terminal o el propio wirehaus-agent). Si macOS lo pide, cierra y vuelve a abrir ese programa y repite este comando.",
                "Grant what is pending in System Settings > Privacy & Security, to the program that launches the agent (the terminal or wirehaus-agent itself). If macOS asks, quit and reopen that program and run this command again."
            )
        )?;
    }
    Ok(complete)
}

pub fn pairing(store: &Store, out: &mut impl Write) -> Result<(), CommandError> {
    let code = store.update(State::open_pairing)??;
    store.log("pairing_opened", "a pairing code was generated");
    let minutes = VALIDITY_SECONDS / 60;
    writeln!(
        out,
        "{}: {}",
        tr!("Código de emparejamiento", "Pairing code"),
        code.to_text()
    )?;
    writeln!(
        out,
        "{}",
        tr!(
            format!(
                "Vale una sola vez y caduca en {minutes} minutos. Para entrar también hará falta el código del segundo factor."
            ),
            format!(
                "It works once and expires in {minutes} minutes. Signing in also needs the second factor code."
            )
        )
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
                    Ok(()) => tr!("activo", "active"),
                    Err(Denied::Suspended) => tr!("suspendido", "suspended"),
                    Err(Denied::Locked { retry_after }) => {
                        tr!(
                            format!("bloqueado {retry_after} s"),
                            format!("locked {retry_after} s")
                        )
                    }
                };
                format!("{} {} ({status})", device.key.to_text(), device.name)
            })
            .collect::<Vec<_>>()
    })?;
    if lines.is_empty() {
        writeln!(
            out,
            "{}",
            tr!(
                "No hay dispositivos emparejados.",
                "There are no paired devices."
            )
        )?;
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
    store.read(|state, _| state.find_by_prefix(prefix))??;
    verify(store, code)?;
    let name = store.update(|state, _| {
        let key = state.find_by_prefix(prefix)?;
        let name = state.device(&key).map(|device| device.name.clone());
        state.remove_device(&key)?;
        Ok::<_, StateError>(name.unwrap_or_default())
    })??;
    store.log("revoked", &name);
    writeln!(
        out,
        "{}",
        tr!(
            format!("Dispositivo revocado: {name}"),
            format!("Device revoked: {name}")
        )
    )?;
    Ok(())
}

pub fn reactivate(
    store: &Store,
    prefix: &str,
    code: &str,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    store.read(|state, _| state.find_by_prefix(prefix))??;
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
    writeln!(
        out,
        "{}",
        tr!(
            format!("Dispositivo reactivado: {name}"),
            format!("Device reactivated: {name}")
        )
    )?;
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
            tr!(
                "Acceso bloqueado: no se aceptan conexiones.",
                "Access locked: connections are refused."
            )
        } else {
            tr!("Acceso desbloqueado.", "Access unlocked.")
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
    let real = folder.canonicalize().map_err(CommandError::Folder)?;
    if !real.is_dir() {
        return Err(CommandError::NotAFolder(real));
    }
    verify(store, code)?;
    let added = store.update(|state, _| state.add_file_root(&real))??;
    if added {
        store.log("files_root_added", &real.display().to_string());
    }
    let location = real.display();
    writeln!(
        out,
        "{}",
        tr!(
            format!("Carpeta compartida: {location}"),
            format!("Shared folder: {location}")
        )
    )?;
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
    let location = target.display();
    writeln!(
        out,
        "{}",
        tr!(
            format!("Carpeta retirada: {location}"),
            format!("Folder removed: {location}")
        )
    )?;
    Ok(())
}

pub fn files_list(store: &Store, out: &mut impl Write) -> Result<(), CommandError> {
    let roots = store.read(|state, _| state.file_roots().to_vec())?;
    if roots.is_empty() {
        writeln!(
            out,
            "{}",
            tr!(
                "No hay carpetas compartidas.",
                "There are no shared folders."
            )
        )?;
    }
    for root in roots {
        writeln!(out, "{}", root.display())?;
    }
    Ok(())
}

pub fn clipboard(
    store: &Store,
    mode: ClipboardMode,
    code: Option<&str>,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    if mode != ClipboardMode::Off {
        verify(store, code.unwrap_or_default())?;
    }
    store.update(|state, _| state.set_clipboard_mode(mode))?;
    store.log(
        match mode {
            ClipboardMode::Off => "clipboard_denied",
            ClipboardMode::ToRemote => "clipboard_to_remote_allowed",
            ClipboardMode::FromRemote => "clipboard_from_remote_allowed",
            ClipboardMode::Both => "clipboard_allowed",
        },
        "by the local administrator",
    );
    writeln!(
        out,
        "{}",
        match mode {
            ClipboardMode::Off => tr!(
                "Portapapeles remoto desactivado.",
                "Remote clipboard turned off."
            ),
            ClipboardMode::ToRemote => tr!(
                "Portapapeles: el visor puede enviar texto a este equipo.",
                "Clipboard: the viewer can send text to this computer."
            ),
            ClipboardMode::FromRemote => tr!(
                "Portapapeles: el visor puede leer el texto de este equipo.",
                "Clipboard: the viewer can read the text on this computer."
            ),
            ClipboardMode::Both => tr!(
                "Portapapeles: el visor puede enviar y leer texto.",
                "Clipboard: the viewer can send and read text."
            ),
        }
    )?;
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
            tr!("Terminal remota permitida.", "Remote terminal allowed.")
        } else {
            tr!(
                "Terminal remota desactivada.",
                "Remote terminal turned off."
            )
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
        .unwrap_or_else(|| "computer".to_owned())
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
    fn the_same_commands_speak_spanish_when_the_locale_says_so() {
        use crate::i18n::{Language, with_language};

        with_language(Language::Spanish, || {
            let (_dir, store, time) = setup();
            let mut out = Vec::new();
            permissions(true, false, &mut out).unwrap();
            let report = text(out);
            assert!(
                report.contains("Grabación de pantalla: concedido"),
                "{report}"
            );
            assert!(report.contains("Accesibilidad): pendiente"), "{report}");
            let mut out = Vec::new();
            pairing(&store, &mut out).unwrap();
            assert!(text(out).contains("Código de emparejamiento: "));
            let mut out = Vec::new();
            devices(&store, &mut out).unwrap();
            let listing = text(out);
            assert!(
                listing.contains("No hay dispositivos emparejados."),
                "{listing:?}"
            );
            let mut out = Vec::new();
            let error = revoke(&store, "000", &code(&store, &time), &mut out).unwrap_err();
            assert_eq!(error.to_string(), "ningún dispositivo coincide");
            assert_eq!(
                CommandError::WrongCode.to_string(),
                "el código es incorrecto"
            );
        });
        assert_eq!(CommandError::WrongCode.to_string(), "the code is wrong");
    }

    #[test]
    fn a_mistake_in_the_target_does_not_use_up_the_code() {
        let (_dir, store, time) = setup();
        add_device(&store, "Aula");
        let valid = code(&store, &time);
        let mut out = Vec::new();
        assert!(matches!(
            files_add(&store, Path::new("/nonexistent/wirehaus"), &valid, &mut out),
            Err(CommandError::Folder(_))
        ));
        assert!(matches!(
            revoke(&store, "000", &valid, &mut out),
            Err(CommandError::State(StateError::NoSuchDevice))
        ));
        assert!(matches!(
            reactivate(&store, "000", &valid, &mut out),
            Err(CommandError::State(StateError::NoSuchDevice))
        ));
        lock(&store, true, &valid, &mut out).unwrap();
    }

    #[test]
    fn the_permissions_report_names_what_is_missing() {
        let mut out = Vec::new();
        assert!(!permissions(true, false, &mut out).unwrap());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Screen recording: granted"));
        assert!(text.contains("(Accessibility): pending"));
        assert!(text.contains("System Settings"));
        let mut done = Vec::new();
        assert!(permissions(true, true, &mut done).unwrap());
        assert!(
            String::from_utf8(done)
                .unwrap()
                .contains("every permission")
        );
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
        assert!(shown.contains("Pairing code: "));
        assert!(shown.contains("10 minutes"));
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
            .trim_start_matches("Pairing code: ");
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
        assert!(text(out).contains("There are no paired devices"));
        let key = add_device(&store, "Aula 3");
        let mut out = Vec::new();
        devices(&store, &mut out).unwrap();
        let shown = text(out);
        assert!(shown.contains(&key));
        assert!(shown.contains("Aula 3 (active)"));
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
        assert!(text(out).contains("Device revoked: Aula 3"));
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
        assert!(text(out).contains("locked"));
        next_step(&time);
        let next = code(&store, &time);
        lock(&store, false, &next, &mut Vec::new()).unwrap();
        assert!(!store.read(|state, _| state.is_locked()).unwrap());
    }

    #[test]
    fn allowing_the_clipboard_needs_a_code_and_denying_it_does_not() {
        let (_dir, store, time) = setup();
        assert!(matches!(
            clipboard(&store, ClipboardMode::Both, Some("000000"), &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        assert!(matches!(
            clipboard(&store, ClipboardMode::ToRemote, None, &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        let mode = |store: &Store| store.read(|state, _| state.clipboard_mode()).unwrap();
        assert_eq!(mode(&store), ClipboardMode::Off);
        let right = code(&store, &time);
        clipboard(
            &store,
            ClipboardMode::FromRemote,
            Some(&right),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(mode(&store), ClipboardMode::FromRemote);
        clipboard(&store, ClipboardMode::Off, None, &mut Vec::new()).unwrap();
        assert_eq!(mode(&store), ClipboardMode::Off);
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
        assert!(text(out).contains("suspended"));
        next_step(&time);
        let right = code(&store, &time);
        reactivate(&store, &key[..12], &right, &mut Vec::new()).unwrap();
        let mut out = Vec::new();
        devices(&store, &mut out).unwrap();
        assert!(text(out).contains("active"));
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
