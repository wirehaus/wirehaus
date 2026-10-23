mod cli;

use std::io::stdout;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use clap::Parser;
use wirehaus_agent::commands;
use wirehaus_agent::{Server, ServerConfig, Store, default_dir, system_clock};
use wirehaus_protocol::PROTOCOL_VERSION;

use wirehaus_agent::state::ClipboardMode;

use crate::cli::{
    Cli, ClipboardAction, Command, DeviceAction, Direction, FilesAction, TerminalAction,
    ViewerAction,
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
        #[cfg(target_os = "macos")]
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
        Command::Viewer { action } => match action {
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
        },
        Command::Clipboard { action } => match action {
            ClipboardAction::Allow { code, direction } => {
                let mode = match direction {
                    Direction::Both => ClipboardMode::Both,
                    Direction::ToRemote => ClipboardMode::ToRemote,
                    Direction::FromRemote => ClipboardMode::FromRemote,
                };
                commands::clipboard(&open()?, mode, Some(&code), &mut out)?;
            }
            ClipboardAction::Deny => {
                commands::clipboard(&open()?, ClipboardMode::Off, None, &mut out)?;
            }
        },
        Command::Files { action } => match action {
            FilesAction::Add { folder, code } => {
                commands::files_add(&open()?, &folder, &code, &mut out)?;
            }
            FilesAction::Remove { folder } => commands::files_remove(&open()?, &folder, &mut out)?,
            FilesAction::List => commands::files_list(&open()?, &mut out)?,
        },
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
        idle_timeout: Duration::from_secs(u64::from(options.idle_minutes) * 60),
        viewer_hash: options.viewer_hash,
        exposure,
    })?;
    println!("listening on http://{}", server.local_addr()?);
    server.run(&AtomicBool::new(false))?;
    Ok(())
}
