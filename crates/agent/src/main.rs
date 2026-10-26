mod cli;

use std::io::stdout;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use clap::Parser;
use wirehaus_agent::commands;
use wirehaus_agent::{Server, ServerConfig, Store, default_dir, system_clock};
use wirehaus_protocol::PROTOCOL_VERSION;

use wirehaus_agent::state::{ClipboardMode, SecretStorage};

use crate::cli::{
    AudioAction, Cli, ClipboardAction, Command, DeviceAction, Direction, FilesAction,
    SecretsAction, SecretsTarget, TerminalAction, ViewerAction,
};

fn main() -> ExitCode {
    wirehaus_agent::i18n::init(wirehaus_agent::i18n::Language::from_environment());
    let cli = Cli::parse();
    let dir = cli.state_dir.clone().unwrap_or_else(default_dir);
    match run(cli.command, dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("wirehaus-agent: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command, dir: std::path::PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let mut out = stdout().lock();
    let open = || Store::open(&dir, system_clock());
    match command {
        Command::Info => {
            println!(
                "wirehaus-agent {} (protocol {})",
                env!("CARGO_PKG_VERSION"),
                PROTOCOL_VERSION
            );
        }
        Command::Init => commands::init(&dir, system_clock(), &mut out)?,
        Command::Pairing => commands::pairing(&open()?, &mut out)?,
        #[cfg(any(target_os = "macos", target_os = "linux", windows))]
        Command::Service { action } => match action {
            crate::cli::ServiceAction::Install { viewer_dir, listen } => {
                wirehaus_agent::service::install(&dir, &viewer_dir, listen, &mut out)?;
            }
            crate::cli::ServiceAction::Uninstall => wirehaus_agent::service::uninstall(&mut out)?,
            crate::cli::ServiceAction::Status => wirehaus_agent::service::status(&mut out)?,
        },
        #[cfg(target_os = "macos")]
        Command::Permissions => {
            let screen = wirehaus_capture::screen_capture_allowed()
                || wirehaus_capture::request_screen_capture();
            let input = wirehaus_input::input_allowed() || wirehaus_input::request_input();
            commands::permissions(screen, input, &mut out)?;
        }
        Command::Devices { action: None } => commands::devices(&open()?, &mut out)?,
        Command::Devices {
            action: Some(DeviceAction::Revoke { device, code }),
        } => commands::revoke(&open()?, &device, &code, &mut out)?,
        Command::Devices {
            action: Some(DeviceAction::Reactivate { device, code }),
        } => commands::reactivate(&open()?, &device, &code, &mut out)?,
        Command::Terminal {
            action: TerminalAction::Allow { code },
        } => commands::terminal(&open()?, true, Some(&code), &mut out)?,
        Command::Terminal {
            action: TerminalAction::Deny,
        } => commands::terminal(&open()?, false, None, &mut out)?,
        Command::Audio {
            action: AudioAction::Allow { code },
        } => commands::audio(&open()?, true, Some(&code), &mut out)?,
        Command::Audio {
            action: AudioAction::Deny,
        } => commands::audio(&open()?, false, None, &mut out)?,
        Command::Viewer { action } => run_viewer(action)?,
        Command::Clipboard { action } => run_clipboard(&open()?, action, &mut out)?,
        Command::Files { action } => run_files(&open()?, action, &mut out)?,
        Command::Secrets { action } => run_secrets(&open()?, action, &mut out)?,
        Command::Lock { code } => commands::lock(&open()?, true, &code, &mut out)?,
        Command::Unlock { code } => commands::lock(&open()?, false, &code, &mut out)?,
        Command::Serve {
            listen,
            display,
            viewer_dir,
            quality,
            idle_minutes,
            viewer_hash,
            public_hosts,
            viewer_origins,
        } => serve(
            dir,
            ServeOptions {
                listen,
                display,
                viewer_dir,
                quality,
                idle_minutes,
                viewer_hash,
                public_hosts,
                viewer_origins,
            },
            &mut out,
        )?,
    }
    Ok(())
}

fn run_viewer(action: ViewerAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        ViewerAction::Manifest { viewer_dir } => {
            println!(
                "{}",
                wirehaus_agent::static_files::write_manifest(&viewer_dir)?
            );
        }
        ViewerAction::Hash { viewer_dir } => {
            let viewer = wirehaus_agent::static_files::Viewer::open(&viewer_dir, None)?;
            println!("{}", viewer.bundle());
        }
    }
    Ok(())
}

fn run_clipboard(
    store: &Store,
    action: ClipboardAction,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        ClipboardAction::Allow { code, direction } => {
            let mode = match direction {
                Direction::Both => ClipboardMode::Both,
                Direction::ToRemote => ClipboardMode::ToRemote,
                Direction::FromRemote => ClipboardMode::FromRemote,
            };
            commands::clipboard(store, mode, Some(&code), out)?;
        }
        ClipboardAction::Deny => commands::clipboard(store, ClipboardMode::Off, None, out)?,
    }
    Ok(())
}

fn run_files(
    store: &Store,
    action: FilesAction,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        FilesAction::Add { folder, code } => commands::files_add(store, &folder, &code, out)?,
        FilesAction::Remove { folder } => commands::files_remove(store, &folder, out)?,
        FilesAction::List => commands::files_list(store, out)?,
    }
    Ok(())
}

fn run_secrets(
    store: &Store,
    action: SecretsAction,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        SecretsAction::Status => commands::secrets_status(store, out)?,
        SecretsAction::Use { target, code } => {
            let target = match target {
                SecretsTarget::System => SecretStorage::System,
                SecretsTarget::File => SecretStorage::File,
            };
            commands::secrets_use(store, target, code.as_deref(), out)?;
        }
    }
    Ok(())
}

struct ServeOptions {
    listen: std::net::SocketAddr,
    display: Option<String>,
    viewer_dir: std::path::PathBuf,
    quality: u8,
    idle_minutes: u32,
    viewer_hash: Option<String>,
    public_hosts: Vec<String>,
    viewer_origins: Vec<String>,
}

fn serve(
    dir: std::path::PathBuf,
    options: ServeOptions,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        let screen = wirehaus_capture::screen_capture_allowed()
            || wirehaus_capture::request_screen_capture();
        let input = wirehaus_input::input_allowed() || wirehaus_input::request_input();
        commands::permissions(screen, input, out)?;
    }
    #[cfg(windows)]
    wirehaus_agent::preflight(options.display.as_deref())?;
    #[cfg(not(target_os = "macos"))]
    let _ = out;
    let exposure = wirehaus_agent::Exposure::new(&options.public_hosts, &options.viewer_origins)?;
    let server = Server::bind(ServerConfig {
        listen: options.listen,
        display: options.display,
        viewer_dir: options.viewer_dir,
        jpeg_quality: options.quality,
        state_dir: dir,
        clock: None,
        vault: None,
        idle_timeout: Duration::from_secs(u64::from(options.idle_minutes) * 60),
        viewer_hash: options.viewer_hash,
        exposure,
    })?;
    println!("listening on http://{}", server.local_addr()?);
    server.run(&AtomicBool::new(false))?;
    Ok(())
}
