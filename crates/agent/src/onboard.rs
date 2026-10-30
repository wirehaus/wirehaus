use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::time::{Duration, Instant};

use crate::commands::{CommandError, host_name, write_totp};
use crate::i18n::tr;
use crate::state::State;
use crate::store::Store;

const PHONE_VALIDITY: u64 = 1800;
const CARD_VALIDITY_DAYS: u64 = 30;
const WAIT_PHONE: Duration = Duration::from_secs(600);
const POLL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Phone,
    Card,
}

#[derive(Debug)]
pub struct Style {
    color: bool,
}

impl Style {
    #[must_use]
    pub fn detect() -> Self {
        Self {
            color: cfg!(unix)
                && std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none(),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    fn bold(&self, text: &str) -> String {
        self.paint("1", text)
    }

    fn accent(&self, text: &str) -> String {
        self.paint("1;36", text)
    }

    fn good(&self, text: &str) -> String {
        self.paint("1;32", text)
    }

    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }
}

pub trait Keyboard {
    fn interactive(&self) -> bool;
    fn line(&mut self) -> Option<String>;
}

pub struct Tty {
    reader: Option<Box<dyn BufRead>>,
}

impl std::fmt::Debug for Tty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tty")
            .field("interactive", &self.reader.is_some())
            .finish()
    }
}

impl Tty {
    #[must_use]
    pub fn open() -> Self {
        Self {
            reader: terminal_reader(),
        }
    }
}

#[cfg(unix)]
fn terminal_reader() -> Option<Box<dyn BufRead>> {
    if !std::io::stdout().is_terminal() {
        return None;
    }
    let tty = std::fs::File::open("/dev/tty").ok()?;
    Some(Box::new(BufReader::new(tty)))
}

#[cfg(not(unix))]
fn terminal_reader() -> Option<Box<dyn BufRead>> {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        Some(Box::new(BufReader::new(std::io::stdin())))
    } else {
        None
    }
}

impl Keyboard for Tty {
    fn interactive(&self) -> bool {
        self.reader.is_some()
    }

    fn line(&mut self) -> Option<String> {
        let reader = self.reader.as_mut()?;
        let mut text = String::new();
        match reader.read_line(&mut text) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(text.trim().to_owned()),
        }
    }
}

fn rule(style: &Style, out: &mut impl Write) -> std::io::Result<()> {
    writeln!(out, "{}", style.dim(&"-".repeat(64)))
}

fn heading(
    style: &Style,
    out: &mut impl Write,
    step: u8,
    total: u8,
    title: &str,
) -> std::io::Result<()> {
    writeln!(out)?;
    rule(style, out)?;
    writeln!(
        out,
        "{}  {}",
        style.accent(&format!("[{step}/{total}]")),
        style.bold(title)
    )?;
    rule(style, out)
}

fn pause(
    style: &Style,
    keys: &mut impl Keyboard,
    out: &mut impl Write,
    message: &str,
) -> std::io::Result<()> {
    if keys.interactive() {
        write!(out, "{} ", style.dim(message))?;
        out.flush()?;
        let _ = keys.line();
    }
    Ok(())
}

pub fn choose_mode(
    style: &Style,
    keys: &mut impl Keyboard,
    out: &mut impl Write,
) -> std::io::Result<Mode> {
    if !keys.interactive() {
        return Ok(Mode::Phone);
    }
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        style.bold(&tr!(
            "Como quieres entrar a este ordenador cuando estés fuera?",
            "How do you want to get into this computer when you are away?"
        ))
    )?;
    writeln!(
        out,
        "  {} {}",
        style.accent("[1]"),
        tr!(
            "Con móvil (recomendado): lo vinculas con un QR y después entras desde cualquier sitio, tambien sin escribir códigos.",
            "With a phone (recommended): you link it with a QR and then get in from anywhere, without typing codes."
        )
    )?;
    writeln!(
        out,
        "  {} {}",
        style.accent("[2]"),
        tr!(
            "Sin móvil: te apuntas una clave de acceso y la usas desde el ordenador que quieras.",
            "Without a phone: you write down an access key and use it from any computer."
        )
    )?;
    for _ in 0..3 {
        write!(out, "{} ", tr!("Elige 1 o 2 [1]:", "Choose 1 or 2 [1]:"))?;
        out.flush()?;
        match keys.line().as_deref() {
            None | Some("" | "1") => return Ok(Mode::Phone),
            Some("2") => return Ok(Mode::Card),
            Some(_) => {}
        }
    }
    Ok(Mode::Phone)
}

fn device_names(store: &Store) -> Result<Vec<String>, CommandError> {
    Ok(store.read(|state, _| {
        state
            .devices()
            .iter()
            .map(|device| device.key.to_text())
            .collect()
    })?)
}

fn name_of(store: &Store, key: &str) -> Option<String> {
    store
        .read(|state, _| {
            state
                .devices()
                .iter()
                .find(|device| device.key.to_text() == key)
                .map(|device| device.name.clone())
        })
        .ok()
        .flatten()
}

#[derive(Debug)]
pub struct Plan<'a> {
    pub store: &'a Store,
    pub address: Option<&'a str>,
    pub mode: Option<Mode>,
    pub wait: Duration,
}

pub fn run(
    plan: &Plan<'_>,
    style: &Style,
    keys: &mut impl Keyboard,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    let mode = match plan.mode {
        Some(mode) => mode,
        None if keys.interactive() => choose_mode(style, keys, out)?,
        None => return plain(plan, style, keys, out),
    };
    match mode {
        Mode::Phone => phone(plan, style, keys, out),
        Mode::Card => card(plan, style, keys, out),
    }
}

fn authenticator(
    plan: &Plan<'_>,
    style: &Style,
    keys: &mut impl Keyboard,
    out: &mut impl Write,
    step: u8,
    total: u8,
) -> Result<(), CommandError> {
    heading(
        style,
        out,
        step,
        total,
        &tr!("Guarda tu segundo factor", "Save your second factor"),
    )?;
    let (secret, uri) = plan.store.read(|state, _| {
        (
            state.totp().to_base32(),
            state.totp().otpauth_uri("Wirehaus", &host_name()),
        )
    })?;
    writeln!(
        out,
        "{}",
        tr!(
            "Necesitas una app de autenticación. Recomendada: 2FAS (gratis); en iPhone también vale «Contraseñas»; en Android, Aegis.",
            "You need an authenticator app. Recommended: 2FAS (free); on iPhone \"Passwords\" also works; on Android, Aegis."
        )
    )?;
    writeln!(
        out,
        "{}",
        style.dim(&tr!(
            "Con tu móvil vinculado casi no la usarás, pero sirve para entrar desde un ordenador sin móvil y para activar terminal y archivos.",
            "With your phone linked you will rarely need it, but it lets you sign in from a computer without a phone and turn on terminal and files."
        ))
    )?;
    write_totp(out, &secret, &uri)?;
    pause(
        style,
        keys,
        out,
        &tr!(
            "Pulsa Enter cuando la hayas añadido...",
            "Press Enter once you have added it..."
        ),
    )?;
    Ok(())
}

fn plain(
    plan: &Plan<'_>,
    style: &Style,
    keys: &mut impl Keyboard,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    authenticator(plan, style, keys, out, 1, 2)?;
    heading(
        style,
        out,
        2,
        2,
        &tr!("Código de emparejamiento", "Pairing code"),
    )?;
    crate::commands::pairing_custom(plan.store, plan.address, false, None, out)?;
    writeln!(
        out,
        "{}",
        style.dim(&tr!(
            "Sin terminal interactivo no se abre el acceso sin segundo factor. Para vincular tu móvil directamente: wirehaus-agent pairing --phone",
            "Without an interactive terminal the no-second-factor access is not opened. To link your phone directly: wirehaus-agent pairing --phone"
        ))
    )?;
    done(style, out)
}

fn phone(
    plan: &Plan<'_>,
    style: &Style,
    keys: &mut impl Keyboard,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    writeln!(
        out,
        "{}",
        style.bold(&tr!("Entrar con tu móvil", "Signing in with your phone"))
    )?;
    authenticator(plan, style, keys, out, 1, 3)?;
    heading(
        style,
        out,
        2,
        3,
        &tr!("Inicia sesión en tu móvil", "Sign in on your phone"),
    )?;
    let before = device_names(plan.store)?;
    let code = plan
        .store
        .update(|state: &mut State, now| state.open_pairing_with(now, PHONE_VALIDITY, true))??;
    plan.store
        .log("pairing_opened", "a trusted phone pairing was generated");
    match plan.address {
        Some(base) => {
            let link = format!("{base}#pair={}", code.to_text());
            writeln!(
                out,
                "{}",
                tr!(
                    "Abre la cámara del móvil, apunta a este QR y abre el enlace. Entras directamente, sin códigos.",
                    "Open your phone camera, point it at this QR and open the link. You are signed in directly, without codes."
                )
            )?;
            if let Some(qr) = crate::qr::render(&link) {
                write!(out, "{qr}")?;
            }
            writeln!(out, "{link}")?;
        }
        None => {
            writeln!(
                out,
                "{}: {}",
                tr!("Código de emparejamiento", "Pairing code"),
                code.to_text()
            )?;
        }
    }
    writeln!(
        out,
        "{}",
        style.dim(&tr!(
            "Vale 30 minutos y una sola vez. Se genera otro con `wirehaus-agent pairing --phone`.",
            "Valid for 30 minutes and once. Make another with `wirehaus-agent pairing --phone`."
        ))
    )?;
    wait_for_phone(plan, style, out, &before)?;
    install_and_arrive(plan, style, out)
}

fn install_and_arrive(
    plan: &Plan<'_>,
    style: &Style,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    heading(
        style,
        out,
        3,
        3,
        &tr!(
            "Instala la app y llega a clase",
            "Install the app and get to class"
        ),
    )?;
    writeln!(
        out,
        "{}",
        tr!(
            "Instálala en el móvil para tenerla siempre a mano y que se actualice sola:",
            "Install it on your phone so it is always at hand and updates itself:"
        )
    )?;
    writeln!(
        out,
        "  {}",
        tr!(
            "iPhone: abre el enlace en Safari > Compartir > Añadir a pantalla de inicio.",
            "iPhone: open the link in Safari > Share > Add to Home Screen."
        )
    )?;
    writeln!(
        out,
        "  {}",
        tr!(
            "Android: Chrome > menú > Instalar aplicación.",
            "Android: Chrome > menu > Install app."
        )
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        style.bold(&tr!(
            "Cuando llegues al otro ordenador:",
            "When you get to the other computer:"
        ))
    )?;
    if let Some(base) = plan.address {
        writeln!(out, "  1. {} {base}", tr!("Abre", "Open"))?;
    } else {
        writeln!(
            out,
            "  1. {}",
            tr!(
                "Abre tu dirección de Wirehaus",
                "Open your Wirehaus address"
            )
        )?;
    }
    writeln!(
        out,
        "  2. {}",
        tr!(
            "Pulsa «Conectar con mi móvil»: aparece un QR.",
            "Press \"Connect with my phone\": a QR appears."
        )
    )?;
    writeln!(
        out,
        "  3. {}",
        tr!(
            "En la app del móvil: Conectar dispositivo > escanea el QR. Listo, la sesión queda iniciada en ese ordenador.",
            "In the phone app: Connect device > scan the QR. Done, that computer stays signed in."
        )
    )?;
    done(style, out)
}

fn wait_for_phone(
    plan: &Plan<'_>,
    style: &Style,
    out: &mut impl Write,
    before: &[String],
) -> Result<(), CommandError> {
    if plan.wait.is_zero() {
        return Ok(());
    }
    writeln!(
        out,
        "{}",
        style.dim(&tr!(
            "Esperando a tu móvil... (Ctrl+C para saltar este paso)",
            "Waiting for your phone... (Ctrl+C to skip this step)"
        ))
    )?;
    out.flush()?;
    let deadline = Instant::now() + plan.wait.min(WAIT_PHONE);
    while Instant::now() < deadline {
        let now = device_names(plan.store)?;
        if let Some(new) = now.iter().find(|key| !before.contains(key)) {
            let name = name_of(plan.store, new).unwrap_or_default();
            writeln!(
                out,
                "{} {name}",
                style.good(&tr!("[OK] Móvil conectado:", "[OK] Phone connected:"))
            )?;
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
    writeln!(
        out,
        "{}",
        tr!(
            "No ha llegado ningún móvil todavia. Cuando quieras: wirehaus-agent pairing --phone",
            "No phone has arrived yet. Whenever you like: wirehaus-agent pairing --phone"
        )
    )?;
    Ok(())
}

fn card(
    plan: &Plan<'_>,
    style: &Style,
    keys: &mut impl Keyboard,
    out: &mut impl Write,
) -> Result<(), CommandError> {
    writeln!(
        out,
        "{}",
        style.bold(&tr!("Entrar sin móvil", "Signing in without a phone"))
    )?;
    authenticator(plan, style, keys, out, 1, 2)?;
    heading(
        style,
        out,
        2,
        2,
        &tr!("Apunta tu tarjeta de acceso", "Write down your access card"),
    )?;
    let code = plan.store.update(|state: &mut State, now| {
        state.open_pairing_with(now, CARD_VALIDITY_DAYS * 86_400, false)
    })??;
    plan.store
        .log("pairing_opened", "a 30-day access card was generated");
    let address = plan.address.map_or_else(
        || tr!("(tu dirección de Wirehaus)", "(your Wirehaus address)"),
        str::to_owned,
    );
    let lines = [
        tr!("TARJETA DE ACCESO WIREHAUS", "WIREHAUS ACCESS CARD"),
        format!("{}  {address}", tr!("Dirección:", "Address:")),
        format!("{}    {}", tr!("Código:", "Code:"), code.to_text()),
        tr!(
            "Después: los 6 digitos de tu app / gestor de contraseñas",
            "Then: the 6 digits from your app / password manager"
        ),
    ];
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    writeln!(out, "+{}+", "-".repeat(width + 2))?;
    for line in &lines {
        let pad = width - line.chars().count();
        writeln!(out, "| {line}{} |", " ".repeat(pad))?;
    }
    writeln!(out, "+{}+", "-".repeat(width + 2))?;
    writeln!(
        out,
        "{}",
        tr!(
            format!(
                "Apúntala donde quieras (papel, gestor de contraseñas). El codigo vale {CARD_VALIDITY_DAYS} días, entra en un solo ordenador y tambien hace falta el segundo factor: sin los dos, nadie entra."
            ),
            format!(
                "Write it down wherever you like (paper, password manager). The code works for {CARD_VALIDITY_DAYS} days, for one computer, and the second factor is also required: without both, nobody gets in."
            )
        )
    )?;
    writeln!(
        out,
        "{}",
        tr!(
            "En el ordenador de destino: abre la dirección, escribe el codigo y después los 6 digitos. La sesión se queda iniciada.",
            "On the computer you are going to use: open the address, type the code and then the 6 digits. The session stays signed in."
        )
    )?;
    writeln!(
        out,
        "{}",
        style.dim(&tr!(
            "Para otra tarjeta: wirehaus-agent pairing --card",
            "For another card: wirehaus-agent pairing --card"
        ))
    )?;
    done(style, out)
}

fn done(style: &Style, out: &mut impl Write) -> Result<(), CommandError> {
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        style.good(&tr!("[OK] Wirehaus está listo.", "[OK] Wirehaus is ready."))
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Script(Vec<String>, bool);

    impl Keyboard for Script {
        fn interactive(&self) -> bool {
            self.1
        }
        fn line(&mut self) -> Option<String> {
            if self.0.is_empty() {
                None
            } else {
                Some(self.0.remove(0))
            }
        }
    }

    fn plain() -> Style {
        Style { color: false }
    }

    #[test]
    fn without_a_terminal_the_phone_mode_is_the_default() {
        let mut out = Vec::new();
        let mode = choose_mode(&plain(), &mut Script(Vec::new(), false), &mut out).unwrap();
        assert_eq!(mode, Mode::Phone);
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn the_choice_accepts_one_two_and_enter() {
        for (typed, expected) in [("", Mode::Phone), ("1", Mode::Phone), ("2", Mode::Card)] {
            let mut out = Vec::new();
            let mode = choose_mode(
                &plain(),
                &mut Script(vec![typed.to_owned()], true),
                &mut out,
            )
            .unwrap();
            assert_eq!(mode, expected);
        }
        let mut out = Vec::new();
        let mode = choose_mode(
            &plain(),
            &mut Script(vec!["x".into(), "y".into(), "2".into()], true),
            &mut out,
        )
        .unwrap();
        assert_eq!(mode, Mode::Card);
    }

    #[test]
    fn colours_are_only_added_when_asked() {
        assert_eq!(plain().bold("a"), "a");
        assert_eq!(Style { color: true }.bold("a"), "\x1b[1ma\x1b[0m");
    }
}
