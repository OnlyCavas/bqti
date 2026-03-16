#[macro_use]
extern crate tracing;

use std::io;

use bqti::{
    BQTIError,
    cli::{Cli, SubCommand, Torrent},
    torrent::{create, inspect, validate},
};
use clap::Parser;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), BQTIError> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .compact()
        .with_writer(io::stderr)
        .with_env_filter(filter)
        .init();

    let cli = Cli::parse();

    if let Some(subcommand) = cli.subcommand {
        match subcommand {
            SubCommand::Torrent { torrent } => match torrent {
                Torrent::Inspect { torrent } => inspect(torrent, cli.verbose),
                Torrent::Validate { torrent } => validate(torrent),
                Torrent::Create(args) => create(args),
            },
        }?;
    }

    Ok(())
}
