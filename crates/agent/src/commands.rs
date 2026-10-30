use std::io::Write;
use std::path::Path;

use thiserror::Error;
use wirehaus_crypto::lockout::Denied;
use wirehaus_crypto::pairing::VALIDITY_SECONDS;

use crate::i18n::tr;
use crate::state::{AuthError, ClipboardMode, SecretStorage, StateError};
use crate::store::{Clock, Store, StoreError};

const ISSUER: &str = "Wirehaus";
const VIEWER_URL_FILE: &str = "viewer-url";

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
            CommandError::WrongCode => tr!(
                "el código es incorrecto: escribe los seis dígitos que muestra ahora tu aplicación de autenticación (no el código de emparejamiento)",
                "the code is wrong: type the six digits your authenticator app shows right now (not the pairing code)"
            ),
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
    init_with(dir, clock, None, out)
}

pub fn init_quiet(
    dir: &Path,
    clock: Clock,
    everything_in: Option<&Path>,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    let store = Store::initialize(dir, clock)?;
    store.log("init", "agent identity created");
    if let Some(home) = everything_in {
        apply_all(&store, home, out)?;
    }
    Ok(())
}

pub fn init_with(
    dir: &Path,
    clock: Clock,
    everything_in: Option<&Path>,
    out: &mut impl Write,
) -> Result<(), CommandError> {
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
            "Segundo factor: añádelo a tu aplicación de autenticación ahora. Podrás volver a verlo en este ordenador con `wirehaus-agent totp`.",
            "Second factor: add it to your authenticator app now. You can see it again on this computer with `wirehaus-agent totp`."
        )
    )?;
    write_totp(out, &secret, &uri)?;
    if let Some(home) = everything_in
        && let Err(error) = apply_all(&store, home, out)
    {
        writeln!(
            out,
            "{}",
            tr!(
                format!(
                    "Aviso: no se pudieron activar las funciones ({error}). Actívalas luego con `wirehaus-agent allow-all --code SEIS_DÍGITOS`."
                ),
                format!(
                    "Warning: the features could not be turned on ({error}). Turn them on later with `wirehaus-agent allow-all --code SIX_DIGITS`."
                )
            )
        )?;
    }
    Ok(())
}

fn apply_all(store: &Store, home: &Path, out: &mut impl Write) -> Result<(), CommandError> {
    let share = home.canonicalize().ok().filter(|real| real.is_dir());
    store.update(|state, _| {
        state.set_terminal_allowed(true);
        state.set_audio_allowed(true);
        state.set_clipboard_mode(ClipboardMode::Both);
        match &share {
            Some(real) => state.add_file_root(real).map(|_| ()),
            None => Ok(()),
        }
    })??;
    store.log(
        "allowed_everything",
        "files, terminal, clipboard and audio were turned on",
    );
    writeln!(
        out,
        "{}",
        tr!(
            "Activado: archivos de tu carpeta personal, terminal, portapapeles y audio. Se cambia cuando quieras con `wirehaus-agent terminal deny`, `clipboard deny`, `audio deny` o `files remove`.",
            "Turned on: files of your home folder, terminal, clipboard and audio. Change it whenever you like with `wirehaus-agent terminal deny`, `clipboard deny`, `audio deny` or `files remove`."
        )
    )?;
    Ok(())
}

pub fn allow_all(
    store: &Store,
    home: &Path,
    code: &str,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    verify(store, code)?;
    apply_all(store, home, out)
}

pub(crate) fn write_totp(
    out: &mut impl Write,
    secret: &str,
    uri: &str,
) -> Result<(), CommandError> {
    if let Some(qr) = crate::qr::render(uri) {
        writeln!(
            out,
            "{}",
            tr!(
                "Escanea este QR con cualquier aplicación de autenticación (2FAS, Aegis, Google Authenticator, Contraseñas de Apple...):",
                "Scan this QR with any authenticator app (2FAS, Aegis, Google Authenticator, Apple Passwords...):"
            )
        )?;
        write!(out, "{qr}")?;
    }
    writeln!(
        out,
        "{}",
        tr!(
            "Si no puedes escanearlo, escribe el secreto a mano:",
            "If you cannot scan it, type the secret by hand:"
        )
    )?;
    writeln!(out, "{}: {secret}", tr!("Secreto", "Secret"))?;
    writeln!(out, "URI: {uri}")?;
    Ok(())
}

pub fn totp(store: &Store, out: &mut impl Write) -> Result<(), CommandError> {
    let (secret, uri) = store.read(|state, _| {
        (
            state.totp().to_base32(),
            state.totp().otpauth_uri(ISSUER, &host_name()),
        )
    })?;
    store.log(
        "totp_shown",
        "the second factor secret was shown on this computer",
    );
    write_totp(out, &secret, &uri)
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
    pairing_with_link(store, None, out)
}

pub fn pairing_with_link(
    store: &Store,
    viewer_url: Option<&str>,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    pairing_custom(store, viewer_url, false, None, out)
}

pub fn pairing_custom(
    store: &Store,
    viewer_url: Option<&str>,
    trusted: bool,
    days: Option<u32>,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    let validity = match (trusted, days) {
        (true, _) => 1800,
        (false, Some(days)) => u64::from(days) * 86_400,
        (false, None) => VALIDITY_SECONDS,
    };
    let code = store.update(|state, now| state.open_pairing_with(now, validity, trusted))??;
    store.log(
        "pairing_opened",
        if trusted {
            "a trusted pairing code was generated"
        } else {
            "a pairing code was generated"
        },
    );
    writeln!(
        out,
        "{}: {}",
        tr!("Código de emparejamiento", "Pairing code"),
        code.to_text()
    )?;
    let span = if validity >= 86_400 {
        tr!(
            format!("{} días", validity / 86_400),
            format!("{} days", validity / 86_400)
        )
    } else {
        tr!(
            format!("{} minutos", validity / 60),
            format!("{} minutes", validity / 60)
        )
    };
    let second = if trusted {
        tr!(
            "Quien lo abra entra directamente, sin segundo factor: úsalo solo con tu propio móvil, delante de este ordenador.",
            "Whoever opens it signs in directly, without the second factor: use it only with your own phone, in front of this computer."
        )
    } else {
        tr!(
            "Para entrar también hará falta el código del segundo factor.",
            "Signing in also needs the second factor code."
        )
    };
    writeln!(
        out,
        "{}",
        tr!(
            format!("Vale una sola vez y caduca en {span}. {second}"),
            format!("It works once and expires in {span}. {second}")
        )
    )?;
    if let Some(base) = viewer_url {
        let link = format!("{base}#pair={}", code.to_text());
        if let Some(qr) = crate::qr::render(&link) {
            writeln!(
                out,
                "{}",
                tr!(
                    "Para no teclear el código, escanea este QR con el móvil y abre el enlace en el equipo desde el que vas a conectarte (o copia el enlace):",
                    "To avoid typing the code, scan this QR with your phone and open the link on the computer you will connect from (or copy the link):"
                )
            )?;
            write!(out, "{qr}")?;
        }
        writeln!(out, "{link}")?;
    }
    Ok(())
}

pub fn remember_viewer_url(
    dir: &Path,
    public_hosts: &[String],
    viewer_origins: &[String],
) -> std::io::Result<()> {
    let (Some(host), Some(origin)) = (public_hosts.first(), viewer_origins.first()) else {
        return Ok(());
    };
    let name = host.split('.').next().unwrap_or(host);
    std::fs::write(
        dir.join(VIEWER_URL_FILE),
        format!("{}/{name}\n", origin.trim_end_matches('/')),
    )
}

#[must_use]
pub fn viewer_url(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(VIEWER_URL_FILE)).ok()?;
    let line = text.trim();
    (line.starts_with("https://") && !line.contains(['#', ' '])).then(|| line.to_string())
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
        store.log("files_root_added", &plain(&real));
    }
    let location = plain(&real);
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
    store.log("files_root_removed", &plain(&target));
    let location = plain(&target);
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

fn plain(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}");
    }
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
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
        writeln!(out, "{}", plain(&root))?;
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

pub fn audio(
    store: &Store,
    allow: bool,
    code: Option<&str>,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    if allow {
        verify(store, code.unwrap_or_default())?;
    }
    store.update(|state, _| state.set_audio_allowed(allow))?;
    store.log(
        if allow {
            "audio_allowed"
        } else {
            "audio_denied"
        },
        "by the local administrator",
    );
    writeln!(
        out,
        "{}",
        if allow {
            tr!(
                "Audio del equipo permitido: el visor podrá escuchar lo que suena aqui.",
                "Audio allowed: the viewer can listen to what plays here."
            )
        } else {
            tr!("Audio desactivado.", "Audio turned off.")
        }
    )?;
    Ok(())
}

pub fn secrets_status(store: &Store, out: &mut impl Write) -> Result<(), CommandError> {
    let storage = store.storage()?;
    writeln!(
        out,
        "{}",
        match storage {
            SecretStorage::File => tr!(
                "Los secretos del agente están en el fichero de estado (solo legible por su dueño).",
                "The agent secrets are in the state file (readable only by its owner)."
            ),
            SecretStorage::System => tr!(
                "Los secretos del agente están en el almacén de claves del sistema.",
                "The agent secrets are in the system vault."
            ),
        }
    )?;
    Ok(())
}

pub fn secrets_use(
    store: &Store,
    target: SecretStorage,
    code: Option<&str>,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    if target == SecretStorage::File {
        verify(store, code.unwrap_or_default())?;
    }
    let moved = store.move_secrets(target)?;
    if moved {
        store.log(
            match target {
                SecretStorage::File => "secrets_to_file",
                SecretStorage::System => "secrets_to_system",
            },
            "by the local administrator",
        );
    }
    writeln!(
        out,
        "{}",
        match (target, moved) {
            (SecretStorage::System, true) => tr!(
                "Los secretos se han movido al almacén de claves del sistema. Reinicia el agente si está en marcha para que lo compruebe al arrancar.",
                "The secrets were moved to the system vault. Restart the agent if it is running so it checks the vault at startup."
            ),
            (SecretStorage::File, true) => tr!(
                "Los secretos vuelven a estar en el fichero de estado y se han borrado del almacén del sistema.",
                "The secrets are back in the state file and were removed from the system vault."
            ),
            (SecretStorage::System, false) => tr!(
                "Los secretos ya estaban en el almacén de claves del sistema.",
                "The secrets were already in the system vault."
            ),
            (SecretStorage::File, false) => tr!(
                "Los secretos ya estaban en el fichero de estado.",
                "The secrets were already in the state file."
            ),
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

pub(crate) fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty())
        })
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

    fn with_vault() -> (TempDir, Store, Arc<AtomicU64>, wirehaus_vault::MemoryVault) {
        let dir = TempDir::new();
        let time = Arc::new(AtomicU64::new(1_000_000));
        init(&dir.0, clock(&time), &mut Vec::new()).unwrap();
        let vault = wirehaus_vault::MemoryVault::new();
        let source: crate::store::VaultSource = {
            let vault = vault.clone();
            Arc::new(move || Ok(Box::new(vault.clone()) as Box<dyn wirehaus_vault::Vault>))
        };
        let store = Store::open_with(&dir.0, clock(&time), source).unwrap();
        (dir, store, time, vault)
    }

    #[test]
    fn the_secrets_go_to_the_vault_freely_and_come_back_only_with_a_code() {
        let (dir, store, time, vault) = with_vault();
        let mut out = Vec::new();
        secrets_status(&store, &mut out).unwrap();
        assert!(text(out).contains("state file"));

        let mut out = Vec::new();
        secrets_use(&store, SecretStorage::System, None, &mut out).unwrap();
        assert!(text(out).contains("moved to the system vault"));
        assert_eq!(vault.len(), 2);
        let mut out = Vec::new();
        secrets_status(&store, &mut out).unwrap();
        assert!(text(out).contains("in the system vault"));
        let mut out = Vec::new();
        secrets_use(&store, SecretStorage::System, None, &mut out).unwrap();
        assert!(text(out).contains("already in the system vault"));

        assert!(matches!(
            secrets_use(&store, SecretStorage::File, None, &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        assert!(matches!(
            secrets_use(&store, SecretStorage::File, Some("000000"), &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        assert_eq!(store.storage().unwrap(), SecretStorage::System);
        assert_eq!(vault.len(), 2);

        next_step(&time);
        let valid = code(&store, &time);
        let mut out = Vec::new();
        secrets_use(&store, SecretStorage::File, Some(&valid), &mut out).unwrap();
        assert!(text(out).contains("back in the state file"));
        assert!(vault.is_empty());
        let log = std::fs::read_to_string(dir.0.join("access.log")).unwrap();
        assert!(log.contains("secrets_to_system") && log.contains("secrets_to_file"));
    }

    #[test]
    fn a_failing_vault_reports_why_and_changes_nothing() {
        let (dir, _store, time, _vault) = with_vault();
        let broken: crate::store::VaultSource = Arc::new(|| {
            Err(wirehaus_vault::VaultError::Unavailable(
                "no daemon".to_owned(),
            ))
        });
        let store = Store::open_with(&dir.0, clock(&time), broken).unwrap();
        let error = secrets_use(&store, SecretStorage::System, None, &mut Vec::new()).unwrap_err();
        assert!(error.to_string().contains("no daemon"), "{error}");
        assert_eq!(store.storage().unwrap(), SecretStorage::File);
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
            assert!(
                CommandError::WrongCode
                    .to_string()
                    .starts_with("el código es incorrecto: escribe los seis dígitos")
            );
        });
        assert!(
            CommandError::WrongCode
                .to_string()
                .starts_with("the code is wrong: type the six digits")
        );
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
    fn init_with_everything_turns_the_features_on_and_shares_the_home_folder() {
        let dir = TempDir::new();
        let home = TempDir::new();
        std::fs::create_dir_all(&home.0).unwrap();
        let time = Arc::new(AtomicU64::new(1_000_000));
        let mut out = Vec::new();
        init_with(&dir.0, clock(&time), Some(&home.0), &mut out).unwrap();
        assert!(text(out).contains("Turned on"));
        let store = Store::open(&dir.0, clock(&time)).unwrap();
        store
            .read(|state, _| {
                assert!(state.terminal_allowed());
                assert!(state.audio_allowed());
                assert_eq!(state.clipboard_mode(), ClipboardMode::Both);
                assert_eq!(state.file_roots(), [home.0.canonicalize().unwrap()]);
            })
            .unwrap();
    }

    #[test]
    fn a_plain_init_leaves_every_feature_off() {
        let (_dir, store, _time) = setup();
        store
            .read(|state, _| {
                assert!(!state.terminal_allowed());
                assert!(!state.audio_allowed());
                assert_eq!(state.clipboard_mode(), ClipboardMode::Off);
                assert_eq!(state.file_roots().len(), 0);
            })
            .unwrap();
    }

    #[test]
    fn allow_all_needs_the_current_second_factor_and_a_real_home_folder() {
        let (_dir, store, time) = setup();
        let home = TempDir::new();
        std::fs::create_dir_all(&home.0).unwrap();
        assert!(matches!(
            allow_all(&store, &home.0, "000000", &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        let right = code(&store, &time);
        allow_all(&store, &home.0, &right, &mut Vec::new()).unwrap();
        store
            .read(|state, _| {
                assert!(state.terminal_allowed());
                assert_eq!(state.file_roots().len(), 1);
            })
            .unwrap();
    }

    #[test]
    fn the_wrong_code_message_explains_which_code_is_meant() {
        let shown = CommandError::WrongCode.to_string();
        assert!(shown.contains("authenticator"));
        assert!(shown.contains("pairing"));
    }

    #[test]
    fn init_shows_a_scannable_qr_and_the_totp_command_shows_it_again() {
        let dir = TempDir::new();
        let time = Arc::new(AtomicU64::new(0));
        let mut out = Vec::new();
        init(&dir.0, clock(&time), &mut out).unwrap();
        assert!(text(out).contains('\u{2588}'));
        let store = Store::open(&dir.0, clock(&time)).unwrap();
        let secret = store.read(|state, _| state.totp().to_base32()).unwrap();
        let mut again = Vec::new();
        totp(&store, &mut again).unwrap();
        let shown = text(again);
        assert!(shown.contains(&secret));
        assert!(shown.contains('\u{2588}'));
    }

    #[test]
    fn pairing_with_a_viewer_url_prints_a_link_that_carries_the_code() {
        let (_dir, store, _time) = setup();
        let mut out = Vec::new();
        pairing_with_link(&store, Some("https://app.example/zz"), &mut out).unwrap();
        let shown = text(out);
        let code = shown
            .lines()
            .next()
            .unwrap()
            .trim_start_matches("Pairing code: ")
            .to_string();
        assert!(shown.contains(&format!("https://app.example/zz#pair={code}")));
        assert!(shown.contains('\u{2588}'));
    }

    #[test]
    fn the_viewer_url_is_remembered_from_the_first_host_and_origin() {
        let dir = TempDir::new();
        std::fs::create_dir_all(&dir.0).unwrap();
        assert_eq!(viewer_url(&dir.0), None);
        remember_viewer_url(
            &dir.0,
            &["zz-test.wirehaus.dev".to_string()],
            &["https://app.wirehaus.dev/".to_string()],
        )
        .unwrap();
        assert_eq!(
            viewer_url(&dir.0).as_deref(),
            Some("https://app.wirehaus.dev/zz-test")
        );
        std::fs::write(dir.0.join("viewer-url"), "http://insecure/x#y").unwrap();
        assert_eq!(viewer_url(&dir.0), None);
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
    fn allowing_audio_needs_a_code_and_denying_it_does_not() {
        let (_dir, store, time) = setup();
        assert!(matches!(
            audio(&store, true, Some("000000"), &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        assert!(matches!(
            audio(&store, true, None, &mut Vec::new()),
            Err(CommandError::WrongCode)
        ));
        assert!(!store.read(|state, _| state.audio_allowed()).unwrap());
        let right = code(&store, &time);
        audio(&store, true, Some(&right), &mut Vec::new()).unwrap();
        assert!(store.read(|state, _| state.audio_allowed()).unwrap());
        audio(&store, false, None, &mut Vec::new()).unwrap();
        assert!(!store.read(|state, _| state.audio_allowed()).unwrap());
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

    #[test]
    fn the_verbatim_prefix_that_windows_adds_is_not_shown_to_people() {
        assert_eq!(
            plain(Path::new(r"\\?\C:\Users\Ana\Docs")),
            r"C:\Users\Ana\Docs"
        );
        assert_eq!(
            plain(Path::new(r"\\?\UNC\server\share\x")),
            r"\\server\share\x"
        );
        assert_eq!(plain(Path::new("/home/ana/docs")), "/home/ana/docs");
        assert_eq!(plain(Path::new(r"C:\plain")), r"C:\plain");
    }
}
