use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::i18n::tr;

pub const LABEL: &str = "wirehaus.agent";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    pub executable: PathBuf,
    pub state_dir: PathBuf,
    pub viewer_dir: PathBuf,
    pub listen: SocketAddr,
    pub public_hosts: Vec<String>,
    pub viewer_origins: Vec<String>,
    pub log_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("{}", self.describe())]
    Io(#[from] std::io::Error),
    #[error("{}", self.describe())]
    Home,
    #[error("{}", self.describe())]
    Launchctl(String),
    #[error("{}", self.describe())]
    Systemctl(String),
    #[error("{}", self.describe())]
    NotInitialized(PathBuf),
    #[error("{}", self.describe())]
    Viewer(#[from] crate::integrity::IntegrityError),
}

impl ServiceError {
    fn describe(&self) -> String {
        match self {
            ServiceError::Io(error) => tr!(
                format!("no se puede preparar el servicio: {error}"),
                format!("cannot set up the service: {error}")
            ),
            ServiceError::Home => tr!(
                "no se encuentra la carpeta personal del usuario",
                "the user's home folder cannot be found"
            ),
            ServiceError::Launchctl(text) => tr!(
                format!("launchctl ha fallado: {text}"),
                format!("launchctl failed: {text}")
            ),
            ServiceError::Systemctl(text) => tr!(
                format!("systemctl ha fallado: {text}"),
                format!("systemctl failed: {text}")
            ),
            ServiceError::NotInitialized(path) => tr!(
                format!(
                    "el agente no está inicializado en {}; ejecuta primero wirehaus-agent init",
                    path.display()
                ),
                format!(
                    "the agent is not initialized in {}; run wirehaus-agent init first",
                    path.display()
                )
            ),
            ServiceError::Viewer(error) => tr!(
                format!("el visor no es de fiar: {error}"),
                format!("the viewer cannot be trusted: {error}")
            ),
        }
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[must_use]
pub fn arguments(config: &ServiceConfig) -> Vec<String> {
    let mut items = vec![
        path_text(&config.executable),
        "--state-dir".to_owned(),
        path_text(&config.state_dir),
        "serve".to_owned(),
        "--viewer-dir".to_owned(),
        path_text(&config.viewer_dir),
        "--listen".to_owned(),
        config.listen.to_string(),
    ];
    for host in &config.public_hosts {
        items.push("--public-host".to_owned());
        items.push(host.clone());
    }
    for origin in &config.viewer_origins {
        items.push("--viewer-origin".to_owned());
        items.push(origin.clone());
    }
    items
}

#[must_use]
pub fn plist(config: &ServiceConfig) -> String {
    let log = escape(&path_text(&config.log_dir.join("agent.log")));
    let mut lines = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_owned(),
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">".to_owned(),
        "<plist version=\"1.0\">".to_owned(),
        "<dict>".to_owned(),
        "    <key>Label</key>".to_owned(),
        format!("    <string>{LABEL}</string>"),
        "    <key>ProgramArguments</key>".to_owned(),
        "    <array>".to_owned(),
    ];
    lines.extend(
        arguments(config)
            .iter()
            .map(|item| format!("        <string>{}</string>", escape(item))),
    );
    lines.extend(
        [
            "    </array>",
            "    <key>RunAtLoad</key>",
            "    <true/>",
            "    <key>KeepAlive</key>",
            "    <true/>",
            "    <key>ThrottleInterval</key>",
            "    <integer>10</integer>",
            "    <key>ProcessType</key>",
            "    <string>Interactive</string>",
            "    <key>LimitLoadToSessionType</key>",
            "    <string>Aqua</string>",
            "    <key>StandardOutPath</key>",
        ]
        .map(str::to_owned),
    );
    lines.push(format!("    <string>{log}</string>"));
    lines.push("    <key>StandardErrorPath</key>".to_owned());
    lines.push(format!("    <string>{log}</string>"));
    lines.push("</dict>".to_owned());
    lines.push("</plist>".to_owned());
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[must_use]
pub fn plist_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

#[must_use]
pub fn log_dir(home: &Path) -> PathBuf {
    home.join("Library").join("Logs").join("Wirehaus")
}

#[must_use]
pub fn domain(uid: &str) -> String {
    format!("gui/{uid}")
}

#[must_use]
pub fn bootstrap_arguments(uid: &str, plist: &Path) -> Vec<String> {
    vec!["bootstrap".to_owned(), domain(uid), path_text(plist)]
}

#[must_use]
pub fn bootout_arguments(uid: &str) -> Vec<String> {
    vec!["bootout".to_owned(), format!("{}/{LABEL}", domain(uid))]
}

#[must_use]
pub fn print_arguments(uid: &str) -> Vec<String> {
    vec!["print".to_owned(), format!("{}/{LABEL}", domain(uid))]
}

#[must_use]
pub fn running(launchctl_print: &str) -> bool {
    launchctl_print
        .lines()
        .map(str::trim)
        .any(|line| line == "state = running")
}

pub const UNIT: &str = "wirehaus-agent.service";

fn unit_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '%' => out.push_str("%%"),
            '$' => out.push_str("$$"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[must_use]
pub fn systemd_unit(config: &ServiceConfig) -> String {
    let command = arguments(config)
        .iter()
        .map(|item| unit_quote(item))
        .collect::<Vec<_>>()
        .join(" ");
    let log = unit_quote(&path_text(&config.log_dir.join("agent.log")));
    let log = &log[1..log.len() - 1];
    [
        "[Unit]".to_owned(),
        "Description=Wirehaus agent".to_owned(),
        String::new(),
        "[Service]".to_owned(),
        format!("ExecStart={command}"),
        "Restart=on-failure".to_owned(),
        "RestartSec=10".to_owned(),
        "NoNewPrivileges=yes".to_owned(),
        format!("StandardOutput=append:{log}"),
        format!("StandardError=append:{log}"),
        String::new(),
    ]
    .join("\n")
}

#[must_use]
pub fn unit_path(home: &Path) -> PathBuf {
    home.join(".config").join("systemd").join("user").join(UNIT)
}

#[must_use]
pub fn systemd_log_dir(home: &Path) -> PathBuf {
    home.join(".local").join("state").join("wirehaus")
}

#[must_use]
pub fn autostart_path(home: &Path) -> PathBuf {
    home.join(".config")
        .join("autostart")
        .join("wirehaus-agent.desktop")
}

#[must_use]
pub fn autostart_entry() -> String {
    [
        "[Desktop Entry]",
        "Type=Application",
        "Name=Wirehaus agent",
        "Exec=sh -c 'systemctl --user import-environment DISPLAY XAUTHORITY && systemctl --user start wirehaus-agent.service'",
        "NoDisplay=true",
        "X-GNOME-Autostart-enabled=true",
        "",
    ]
    .join("\n")
}

#[must_use]
pub fn systemctl_import_arguments() -> Vec<String> {
    ["--user", "import-environment", "DISPLAY", "XAUTHORITY"]
        .map(str::to_owned)
        .to_vec()
}

#[must_use]
pub fn systemctl_start_arguments() -> Vec<String> {
    ["--user", "start", UNIT].map(str::to_owned).to_vec()
}

#[must_use]
pub fn systemctl_stop_arguments() -> Vec<String> {
    ["--user", "stop", UNIT].map(str::to_owned).to_vec()
}

#[must_use]
pub fn systemctl_active_arguments() -> Vec<String> {
    ["--user", "is-active", UNIT].map(str::to_owned).to_vec()
}

#[must_use]
pub fn systemctl_reload_arguments() -> Vec<String> {
    ["--user", "daemon-reload"].map(str::to_owned).to_vec()
}

#[must_use]
pub fn unit_active(systemctl_is_active: &str) -> bool {
    systemctl_is_active.trim() == "active"
}

#[cfg(target_os = "macos")]
pub use launchd::{install, status, uninstall};

#[cfg(target_os = "linux")]
pub use systemd::{install, status, uninstall};

#[cfg(target_os = "macos")]
mod launchd {
    use std::io::Write;
    use std::net::SocketAddr;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{
        LABEL, ServiceConfig, ServiceError, bootout_arguments, bootstrap_arguments, log_dir, plist,
        plist_path, print_arguments, running,
    };
    use crate::i18n::tr;
    use crate::static_files::Viewer;
    use crate::store::{Store, system_clock};

    fn home() -> Result<PathBuf, ServiceError> {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .ok_or(ServiceError::Home)
    }

    fn uid() -> Result<String, ServiceError> {
        let output = Command::new("/usr/bin/id").arg("-u").output()?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn launchctl(arguments: &[String]) -> Result<(bool, String), ServiceError> {
        let output = Command::new("/bin/launchctl").args(arguments).output()?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok((output.status.success(), text.trim().to_owned()))
    }

    pub fn install(
        state_dir: &Path,
        viewer_dir: &Path,
        listen: SocketAddr,
        public_hosts: &[String],
        viewer_origins: &[String],
        out: &mut impl Write,
    ) -> Result<(), ServiceError> {
        let home = home()?;
        let uid = uid()?;
        let state_dir = std::fs::canonicalize(state_dir)
            .map_err(|_| ServiceError::NotInitialized(state_dir.to_path_buf()))?;
        Store::open(&state_dir, system_clock())
            .map_err(|_| ServiceError::NotInitialized(state_dir.clone()))?;
        let viewer_dir = std::fs::canonicalize(viewer_dir)?;
        Viewer::open(&viewer_dir, None)?;
        let config = ServiceConfig {
            executable: std::fs::canonicalize(std::env::current_exe()?)?,
            state_dir,
            viewer_dir,
            listen,
            public_hosts: public_hosts.to_vec(),
            viewer_origins: viewer_origins.to_vec(),
            log_dir: log_dir(&home),
        };
        std::fs::create_dir_all(&config.log_dir)?;
        let path = plist_path(&home);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = launchctl(&bootout_arguments(&uid))?;
        std::fs::write(&path, plist(&config))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
        let (loaded, text) = launchctl(&bootstrap_arguments(&uid, &path))?;
        if !loaded {
            return Err(ServiceError::Launchctl(text));
        }
        let location = path.display();
        writeln!(
            out,
            "{}",
            tr!(
                format!("Servicio {LABEL} instalado en {location}."),
                format!("Service {LABEL} installed in {location}.")
            )
        )?;
        writeln!(
            out,
            "{}: {}",
            tr!("Registro", "Log"),
            config.log_dir.join("agent.log").display()
        )?;
        writeln!(
            out,
            "{}",
            tr!(
                "Arranca solo al iniciar sesión y se reinicia si se cae. La primera vez macOS pedirá los permisos de Grabación de pantalla y Accesibilidad para wirehaus-agent; concédelos en Ajustes del Sistema > Privacidad y seguridad.",
                "It starts by itself when you log in and restarts if it dies. The first time, macOS will ask for the Screen Recording and Accessibility permissions for wirehaus-agent; grant them in System Settings > Privacy & Security."
            )
        )?;
        Ok(())
    }

    pub fn uninstall(out: &mut impl Write) -> Result<(), ServiceError> {
        let home = home()?;
        let uid = uid()?;
        let _ = launchctl(&bootout_arguments(&uid))?;
        let path = plist_path(&home);
        let existed = path.exists();
        if existed {
            std::fs::remove_file(&path)?;
        }
        if existed {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("Servicio {LABEL} desinstalado."),
                    format!("Service {LABEL} uninstalled.")
                )
            )?;
        } else {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("El servicio {LABEL} no estaba instalado."),
                    format!("Service {LABEL} was not installed.")
                )
            )?;
        }
        Ok(())
    }

    pub fn status(out: &mut impl Write) -> Result<(), ServiceError> {
        let home = home()?;
        let uid = uid()?;
        if !plist_path(&home).exists() {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("El servicio {LABEL} no está instalado."),
                    format!("Service {LABEL} is not installed.")
                )
            )?;
            return Ok(());
        }
        let (loaded, text) = launchctl(&print_arguments(&uid))?;
        if loaded && running(&text) {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("El servicio {LABEL} está en marcha."),
                    format!("Service {LABEL} is running.")
                )
            )?;
        } else if loaded {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("El servicio {LABEL} está cargado pero parado; mira el registro."),
                    format!("Service {LABEL} is loaded but stopped; check the log.")
                )
            )?;
        } else {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("El servicio {LABEL} está instalado pero no cargado."),
                    format!("Service {LABEL} is installed but not loaded.")
                )
            )?;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod systemd {
    use std::io::Write;
    use std::net::SocketAddr;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{
        ServiceConfig, ServiceError, UNIT, autostart_entry, autostart_path,
        systemctl_active_arguments, systemctl_import_arguments, systemctl_reload_arguments,
        systemctl_start_arguments, systemctl_stop_arguments, systemd_log_dir, systemd_unit,
        unit_active, unit_path,
    };
    use crate::i18n::tr;
    use crate::static_files::Viewer;
    use crate::store::{Store, system_clock};

    fn home() -> Result<PathBuf, ServiceError> {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .ok_or(ServiceError::Home)
    }

    fn systemctl(arguments: &[String]) -> Result<(bool, String), ServiceError> {
        let output = Command::new("/usr/bin/systemctl")
            .args(arguments)
            .output()?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok((output.status.success(), text.trim().to_owned()))
    }

    pub fn install(
        state_dir: &Path,
        viewer_dir: &Path,
        listen: SocketAddr,
        public_hosts: &[String],
        viewer_origins: &[String],
        out: &mut impl Write,
    ) -> Result<(), ServiceError> {
        let home = home()?;
        let state_dir = std::fs::canonicalize(state_dir)
            .map_err(|_| ServiceError::NotInitialized(state_dir.to_path_buf()))?;
        Store::open(&state_dir, system_clock())
            .map_err(|_| ServiceError::NotInitialized(state_dir.clone()))?;
        let viewer_dir = std::fs::canonicalize(viewer_dir)?;
        Viewer::open(&viewer_dir, None)?;
        let config = ServiceConfig {
            executable: std::fs::canonicalize(std::env::current_exe()?)?,
            state_dir,
            viewer_dir,
            listen,
            public_hosts: public_hosts.to_vec(),
            viewer_origins: viewer_origins.to_vec(),
            log_dir: systemd_log_dir(&home),
        };
        std::fs::create_dir_all(&config.log_dir)?;
        let path = unit_path(&home);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, systemd_unit(&config))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
        let (reloaded, text) = systemctl(&systemctl_reload_arguments())?;
        if !reloaded {
            return Err(ServiceError::Systemctl(text));
        }
        let autostart = autostart_path(&home);
        if let Some(parent) = autostart.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&autostart, autostart_entry())?;
        std::fs::set_permissions(&autostart, std::fs::Permissions::from_mode(0o644))?;
        let in_session = std::env::var_os("DISPLAY").is_some();
        if in_session {
            let _ = systemctl(&systemctl_stop_arguments())?;
            let (imported, text) = systemctl(&systemctl_import_arguments())?;
            if !imported {
                return Err(ServiceError::Systemctl(text));
            }
            let (started, text) = systemctl(&systemctl_start_arguments())?;
            if !started {
                return Err(ServiceError::Systemctl(text));
            }
        }
        let location = path.display();
        writeln!(
            out,
            "{}",
            tr!(
                format!("Servicio {UNIT} instalado en {location}."),
                format!("Service {UNIT} installed in {location}.")
            )
        )?;
        writeln!(
            out,
            "{}: {}",
            tr!("Registro", "Log"),
            config.log_dir.join("agent.log").display()
        )?;
        writeln!(
            out,
            "{}",
            tr!(
                "Arranca solo al iniciar sesión en el escritorio y se reinicia si se cae. Necesita una sesión de escritorio X11 abierta; si no hay ninguna ahora, arrancará en el próximo inicio de sesión.",
                "It starts by itself when you sign in to the desktop and restarts if it dies. It needs an open X11 desktop session; if there is none now, it will start at your next sign-in."
            )
        )?;
        Ok(())
    }

    pub fn uninstall(out: &mut impl Write) -> Result<(), ServiceError> {
        let home = home()?;
        let path = unit_path(&home);
        let existed = path.exists();
        let autostart = autostart_path(&home);
        if autostart.exists() {
            std::fs::remove_file(&autostart)?;
        }
        if existed {
            let _ = systemctl(&systemctl_stop_arguments())?;
            std::fs::remove_file(&path)?;
            let _ = systemctl(&systemctl_reload_arguments())?;
        }
        if existed {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("Servicio {UNIT} desinstalado."),
                    format!("Service {UNIT} uninstalled.")
                )
            )?;
        } else {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("El servicio {UNIT} no estaba instalado."),
                    format!("Service {UNIT} was not installed.")
                )
            )?;
        }
        Ok(())
    }

    pub fn status(out: &mut impl Write) -> Result<(), ServiceError> {
        let home = home()?;
        if !unit_path(&home).exists() {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("El servicio {UNIT} no está instalado."),
                    format!("Service {UNIT} is not installed.")
                )
            )?;
            return Ok(());
        }
        let (_, text) = systemctl(&systemctl_active_arguments())?;
        if unit_active(&text) {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("El servicio {UNIT} está en marcha."),
                    format!("Service {UNIT} is running.")
                )
            )?;
        } else {
            writeln!(
                out,
                "{}",
                tr!(
                    format!("El servicio {UNIT} está instalado pero parado; mira el registro."),
                    format!("Service {UNIT} is installed but stopped; check the log.")
                )
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ServiceConfig {
        ServiceConfig {
            executable: PathBuf::from("/Applications/Wirehaus/wirehaus-agent"),
            state_dir: PathBuf::from("/Users/ana/.config/wirehaus"),
            viewer_dir: PathBuf::from("/Applications/Wirehaus/viewer"),
            listen: "127.0.0.1:7878".parse().unwrap(),
            public_hosts: Vec::new(),
            viewer_origins: Vec::new(),
            log_dir: PathBuf::from("/Users/ana/Library/Logs/Wirehaus"),
        }
    }

    #[test]
    fn the_agent_is_started_with_explicit_paths_and_a_loopback_address() {
        assert_eq!(
            arguments(&config()),
            [
                "/Applications/Wirehaus/wirehaus-agent",
                "--state-dir",
                "/Users/ana/.config/wirehaus",
                "serve",
                "--viewer-dir",
                "/Applications/Wirehaus/viewer",
                "--listen",
                "127.0.0.1:7878"
            ]
        );
    }

    #[test]
    fn the_public_host_and_viewer_origin_reach_the_service_command_line() {
        let mut settings = config();
        settings.public_hosts = vec!["ana.wirehaus.dev".to_owned()];
        settings.viewer_origins = vec!["https://app.wirehaus.dev".to_owned()];
        let items = arguments(&settings);
        assert_eq!(
            items[items.len() - 4..],
            [
                "--public-host",
                "ana.wirehaus.dev",
                "--viewer-origin",
                "https://app.wirehaus.dev"
            ]
        );
        assert!(
            !arguments(&config())
                .iter()
                .any(|item| item.starts_with("--public"))
        );
    }

    #[test]
    fn the_plist_names_the_job_keeps_it_alive_and_limits_it_to_the_graphical_session() {
        let text = plist(&config());
        assert!(text.starts_with("<?xml version=\"1.0\""));
        assert!(text.contains("<key>Label</key>\n    <string>wirehaus.agent</string>"));
        assert!(text.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(text.contains("<key>KeepAlive</key>\n    <true/>"));
        assert!(text.contains("<key>LimitLoadToSessionType</key>\n    <string>Aqua</string>"));
        assert!(text.contains("<string>/Users/ana/Library/Logs/Wirehaus/agent.log</string>"));
        assert!(text.contains("        <string>serve</string>\n"));
        assert!(text.ends_with("</plist>\n"));
    }

    #[test]
    fn paths_with_markup_characters_cannot_break_out_of_the_plist() {
        let mut config = config();
        config.viewer_dir = PathBuf::from("/tmp/a&b</string><string>evil\"'");
        let text = plist(&config);
        assert!(text.contains("/tmp/a&amp;b&lt;/string&gt;&lt;string&gt;evil&quot;&apos;"));
        assert!(!text.contains("<string>evil"));
        assert_eq!(
            text.matches("<string>").count(),
            text.matches("</string>").count()
        );
    }

    #[test]
    fn launchctl_arguments_target_the_users_graphical_domain() {
        let path = plist_path(Path::new("/Users/ana"));
        assert_eq!(
            path,
            PathBuf::from("/Users/ana/Library/LaunchAgents/wirehaus.agent.plist")
        );
        assert_eq!(
            bootstrap_arguments("501", &path),
            [
                "bootstrap",
                "gui/501",
                "/Users/ana/Library/LaunchAgents/wirehaus.agent.plist"
            ]
        );
        assert_eq!(
            bootout_arguments("501"),
            ["bootout", "gui/501/wirehaus.agent"]
        );
        assert_eq!(print_arguments("501"), ["print", "gui/501/wirehaus.agent"]);
        assert_eq!(
            log_dir(Path::new("/Users/ana")),
            PathBuf::from("/Users/ana/Library/Logs/Wirehaus")
        );
    }

    #[test]
    fn the_running_state_is_read_from_the_launchctl_report() {
        assert!(running(
            "wirehaus.agent = {\n\tstate = running\n\tpid = 1\n}"
        ));
        assert!(!running("wirehaus.agent = {\n\tstate = waiting\n}"));
        assert!(!running("Could not find service"));
        assert!(!running("\tlast exit state = running"));
    }

    #[test]
    fn the_unit_restarts_on_failure_and_is_started_by_the_autostart_entry_not_by_a_target() {
        let text = systemd_unit(&config());
        assert!(text.starts_with("[Unit]\n"));
        assert!(!text.contains("[Install]"));
        assert!(text.contains("Restart=on-failure\n"));
        assert!(text.contains("NoNewPrivileges=yes\n"));
        assert!(text.contains(
            "ExecStart=\"/Applications/Wirehaus/wirehaus-agent\" \"--state-dir\" \"/Users/ana/.config/wirehaus\" \"serve\""
        ));
        assert!(
            text.contains("StandardOutput=append:/Users/ana/Library/Logs/Wirehaus/agent.log\n")
        );
    }

    #[test]
    fn the_autostart_entry_hands_the_desktop_environment_to_the_user_manager_before_starting() {
        let text = autostart_entry();
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains(
            "import-environment DISPLAY XAUTHORITY && systemctl --user start wirehaus-agent.service"
        ));
        assert!(text.contains("NoDisplay=true\n"));
        assert_eq!(
            autostart_path(Path::new("/home/ana")),
            PathBuf::from("/home/ana/.config/autostart/wirehaus-agent.desktop")
        );
    }

    #[test]
    fn paths_with_unit_syntax_characters_cannot_change_the_command() {
        let mut odd = config();
        odd.executable = PathBuf::from("/opt/a b/\"x\"/$HOME/100%");
        let text = systemd_unit(&odd);
        assert!(text.contains("ExecStart=\"/opt/a b/\\\"x\\\"/$$HOME/100%%\" "));
        assert_eq!(text.matches("ExecStart=").count(), 1);
    }

    #[test]
    fn systemctl_acts_on_the_user_manager_and_the_unit_path_is_under_the_config_folder() {
        assert_eq!(
            systemctl_start_arguments(),
            ["--user", "start", "wirehaus-agent.service"]
        );
        assert_eq!(
            systemctl_stop_arguments(),
            ["--user", "stop", "wirehaus-agent.service"]
        );
        assert_eq!(
            unit_path(Path::new("/home/ana")),
            PathBuf::from("/home/ana/.config/systemd/user/wirehaus-agent.service")
        );
        assert_eq!(
            systemd_log_dir(Path::new("/home/ana")),
            PathBuf::from("/home/ana/.local/state/wirehaus")
        );
    }

    #[test]
    fn only_the_active_state_counts_as_running() {
        assert!(unit_active("active\n"));
        assert!(!unit_active("inactive\n"));
        assert!(!unit_active("failed\n"));
        assert!(!unit_active(""));
    }
}
