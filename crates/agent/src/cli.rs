use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

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
    Init,
    Pairing,
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
        #[arg(long)]
        code: String,
    },
    Unlock {
        #[arg(long)]
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
        #[arg(long)]
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
        #[arg(long)]
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
        #[arg(long)]
        code: String,
    },
    Deny,
}

#[derive(Debug, Subcommand)]
pub enum AudioAction {
    Allow {
        #[arg(long)]
        code: String,
    },
    Deny,
}

#[derive(Debug, Subcommand)]
pub enum DeviceAction {
    Revoke {
        device: String,
        #[arg(long)]
        code: String,
    },
    Reactivate {
        device: String,
        #[arg(long)]
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
