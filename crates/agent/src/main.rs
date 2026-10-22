mod cli;

use std::io::stdout;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use clap::Parser;
use wirehaus_agent::commands;
use wirehaus_agent::{Server, ServerConfig, Store, default_dir, system_clock};
use wirehaus_protocol::PROTOCOL_VERSION;

use wirehaus_agent::state::ClipboardMode;

use crate::cli::{
    Cli, ClipboardAction, Command, DeviceAction, Direction, FilesAction, TerminalAction,
};

fn main() -> ExitCode {
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
        } => {
            let server = Server::bind(ServerConfig {
                listen,
                display,
                viewer_dir,
                jpeg_quality: quality,
                state_dir: dir,
                clock: None,
            })?;
            println!("listening on http://{}", server.local_addr()?);
            server.run(&AtomicBool::new(false))?;
        }
    }
    Ok(())
}
