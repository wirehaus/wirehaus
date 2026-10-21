mod cli;

use clap::Parser;
use wirehaus_protocol::PROTOCOL_VERSION;

use crate::cli::{Cli, Command};

fn main() {
    match Cli::parse().command {
        Command::Info => println!(
            "wirehaus-agent {} (protocol {})",
            env!("CARGO_PKG_VERSION"),
            PROTOCOL_VERSION
        ),
    }
}
