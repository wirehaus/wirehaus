use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
    Devices {
        #[command(subcommand)]
        action: Option<DeviceAction>,
    },
    Terminal {
        #[command(subcommand)]
        action: TerminalAction,
    },
    Files {
        #[command(subcommand)]
        action: FilesAction,
    },
    Lock {
        #[arg(long)]
        code: String,
    },
    Unlock {
        #[arg(long)]
        code: String,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1:7878")]
        listen: SocketAddr,
        #[arg(long)]
        display: Option<String>,
        #[arg(long, default_value = "viewer/dist")]
        viewer_dir: PathBuf,
        #[arg(long, default_value_t = 70, value_parser = clap::value_parser!(u8).range(1..=100))]
        quality: u8,
    },
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
