use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

const CODE_HELP: &str =
    "The six digits your authenticator app shows right now (not the pairing code)";

#[derive(Debug, Parser)]
#[command(name = "wirehaus-agent", version, about = "Wirehaus agent")]
pub struct Cli {
    #[arg(long, global = true, value_name = "DIR")]
    pub state_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Info,
    Init {
        #[arg(long)]
        allow_all: bool,
        #[arg(long)]
        quiet: bool,
    },
    Onboard {
        #[arg(long, conflicts_with = "card")]
        phone: bool,
        #[arg(long)]
        card: bool,
        #[arg(long)]
        no_wait: bool,
    },
    AllowAll {
        #[arg(long, value_name = "SIX DIGITS", help = CODE_HELP)]
        code: String,
    },
    Pairing {
        #[arg(long, conflicts_with_all = ["card", "days"])]
        phone: bool,
        #[arg(long, conflicts_with = "days")]
        card: bool,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=90))]
        days: Option<u32>,
    },
    Totp,
    #[cfg(target_os = "macos")]
    Permissions,
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    Devices {
        #[command(subcommand)]
        action: Option<DeviceAction>,
    },
    Terminal {
        #[command(subcommand)]
        action: TerminalAction,
    },
    Clipboard {
        #[command(subcommand)]
        action: ClipboardAction,
    },
    Audio {
        #[command(subcommand)]
        action: AudioAction,
    },
    Viewer {
        #[command(subcommand)]
        action: ViewerAction,
    },
    Files {
        #[command(subcommand)]
        action: FilesAction,
    },
    Secrets {
        #[command(subcommand)]
        action: SecretsAction,
    },
    Lock {
        #[arg(long, value_name = "SIX DIGITS", help = CODE_HELP)]
        code: String,
    },
    Unlock {
        #[arg(long, value_name = "SIX DIGITS", help = CODE_HELP)]
        code: String,
    },
    Update {
        #[command(subcommand)]
        action: UpdateAction,
    },
    Serve {
        #[arg(long)]
        no_auto_update: bool,
        #[arg(long, default_value = "127.0.0.1:7878")]
        listen: SocketAddr,
        #[arg(long)]
        display: Option<String>,
        #[arg(long, default_value = "viewer/dist")]
        viewer_dir: PathBuf,
        #[arg(long, default_value_t = 70, value_parser = clap::value_parser!(u8).range(1..=100))]
        quality: u8,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..=1440))]
        idle_minutes: u32,
        #[arg(long, default_value_t = wirehaus_agent::connector::DEFAULT_SLEEP_MINUTES, value_parser = clap::value_parser!(u32).range(0..=1440))]
        tunnel_sleep_minutes: u32,
        #[arg(long, value_name = "SHA256")]
        viewer_hash: Option<String>,
        #[arg(long = "public-host", value_name = "HOST")]
        public_hosts: Vec<String>,
        #[arg(long = "viewer-origin", value_name = "URL")]
        viewer_origins: Vec<String>,
    },
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
#[derive(Debug, Subcommand)]
pub enum ServiceAction {
    Install {
        #[arg(long, default_value = "viewer/dist")]
        viewer_dir: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7878")]
        listen: SocketAddr,
        #[arg(long = "public-host", value_name = "HOST")]
        public_hosts: Vec<String>,
        #[arg(long = "viewer-origin", value_name = "URL")]
        viewer_origins: Vec<String>,
    },
    Uninstall,
    Status,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum UpdateAction {
    Status,
    Check,
    Apply,
    Enable,
    Disable,
    Rollback,
}

#[derive(Debug, Subcommand)]
pub enum ViewerAction {
    Manifest {
        #[arg(long, default_value = "viewer/dist")]
        viewer_dir: PathBuf,
    },
    Hash {
        #[arg(long, default_value = "viewer/dist")]
        viewer_dir: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Direction {
    Both,
    ToRemote,
    FromRemote,
}

#[derive(Debug, Subcommand)]
pub enum ClipboardAction {
    Allow {
        #[arg(long, value_name = "SIX DIGITS", help = CODE_HELP)]
        code: String,
        #[arg(long, value_enum, default_value_t = Direction::Both)]
        direction: Direction,
    },
    Deny,
}

#[derive(Debug, Subcommand)]
pub enum FilesAction {
    Add {
        folder: PathBuf,
        #[arg(long, value_name = "SIX DIGITS", help = CODE_HELP)]
        code: String,
    },
    Remove {
        folder: PathBuf,
    },
    List,
}

#[derive(Debug, Subcommand)]
pub enum TerminalAction {
    Allow {
        #[arg(long, value_name = "SIX DIGITS", help = CODE_HELP)]
        code: String,
    },
    Deny,
}

#[derive(Debug, Subcommand)]
pub enum AudioAction {
    Allow {
        #[arg(long, value_name = "SIX DIGITS", help = CODE_HELP)]
        code: String,
    },
    Deny,
}

#[derive(Debug, Subcommand)]
pub enum DeviceAction {
    Revoke {
        device: String,
        #[arg(long, value_name = "SIX DIGITS", help = CODE_HELP)]
        code: String,
    },
    Reactivate {
        device: String,
        #[arg(long, value_name = "SIX DIGITS", help = CODE_HELP)]
        code: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SecretsTarget {
    System,
    File,
}

#[derive(Debug, Subcommand)]
pub enum SecretsAction {
    Status,
    Use {
        #[arg(value_enum)]
        target: SecretsTarget,
        #[arg(long)]
        code: Option<String>,
    },
}
