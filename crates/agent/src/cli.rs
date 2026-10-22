use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "wirehaus-agent", version, about = "Wirehaus agent")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Info,
    Serve {
        #[arg(long, default_value = "127.0.0.1:7878")]
        listen: SocketAddr,
        #[arg(long)]
        display: Option<String>,
        #[arg(long, default_value = "web/dist")]
        viewer_dir: PathBuf,
        #[arg(long, default_value_t = 70, value_parser = clap::value_parser!(u8).range(1..=100))]
        quality: u8,
    },
}
