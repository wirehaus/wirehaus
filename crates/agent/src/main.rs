mod cli;

use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use clap::Parser;
use wirehaus_agent::{Server, ServerConfig};
use wirehaus_protocol::PROTOCOL_VERSION;

use crate::cli::{Cli, Command};

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Info => {
            println!(
                "wirehaus-agent {} (protocol {})",
                env!("CARGO_PKG_VERSION"),
                PROTOCOL_VERSION
            );
            ExitCode::SUCCESS
        }
        Command::Serve {
            listen,
            display,
            viewer_dir,
            quality,
        } => {
            let config = ServerConfig {
                listen,
                display,
                viewer_dir,
                jpeg_quality: quality,
            };
            match serve(config) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("wirehaus-agent: {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn serve(config: ServerConfig) -> Result<(), wirehaus_agent::ServerError> {
    let server = Server::bind(config)?;
    println!("listening on http://{}", server.local_addr()?);
    server.run(&AtomicBool::new(false))
}
