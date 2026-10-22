use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use thiserror::Error;

pub const LABEL: &str = "wirehaus.agent";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    pub executable: PathBuf,
    pub state_dir: PathBuf,
    pub viewer_dir: PathBuf,
    pub listen: SocketAddr,
    pub log_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("no se puede preparar el servicio: {0}")]
    Io(#[from] std::io::Error),
    #[error("no se encuentra la carpeta personal del usuario")]
    Home,
    #[error("launchctl ha fallado: {0}")]
    Launchctl(String),
    #[error("el agente no esta inicializado en {0}; ejecuta primero wirehaus-agent init")]
    NotInitialized(PathBuf),
    #[error("el visor no es de fiar: {0}")]
    Viewer(#[from] crate::integrity::IntegrityError),
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
    vec![
        path_text(&config.executable),
        "--state-dir".to_owned(),
        path_text(&config.state_dir),
        "serve".to_owned(),
        "--viewer-dir".to_owned(),
        path_text(&config.viewer_dir),
        "--listen".to_owned(),
        config.listen.to_string(),
    ]
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

#[cfg(target_os = "macos")]
pub use launchd::{install, status, uninstall};

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
        writeln!(out, "Servicio {LABEL} instalado en {}.", path.display())?;
        writeln!(
            out,
            "Registro: {}",
            config.log_dir.join("agent.log").display()
        )?;
        writeln!(
            out,
            "Arranca solo al iniciar sesion y se reinicia si se cae. La primera vez macOS pedira los permisos de Grabacion de pantalla y Accesibilidad para wirehaus-agent; concedelos en Ajustes del Sistema > Privacidad y seguridad."
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
            writeln!(out, "Servicio {LABEL} desinstalado.")?;
        } else {
            writeln!(out, "El servicio {LABEL} no estaba instalado.")?;
        }
        Ok(())
    }

    pub fn status(out: &mut impl Write) -> Result<(), ServiceError> {
        let home = home()?;
        let uid = uid()?;
        if !plist_path(&home).exists() {
            writeln!(out, "El servicio {LABEL} no esta instalado.")?;
            return Ok(());
        }
        let (loaded, text) = launchctl(&print_arguments(&uid))?;
        if loaded && running(&text) {
            writeln!(out, "El servicio {LABEL} esta en marcha.")?;
        } else if loaded {
            writeln!(
                out,
                "El servicio {LABEL} esta cargado pero parado; mira el registro."
            )?;
        } else {
            writeln!(out, "El servicio {LABEL} esta instalado pero no cargado.")?;
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
}
