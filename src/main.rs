use std::io;

use anyhow::Result;
use bqti::{
    certs,
    cli::{Cli, SubCommand},
    download, seed, serve,
    torrent::{Torrent, create, inspect, validate},
};

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));

    tracing_subscriber::fmt()
        .compact()
        .with_writer(io::stderr)
        .with_env_filter(filter)
        .init();

    let cli = Cli::parse();

    if let Some(subcommand) = cli.subcommand {
        match subcommand {
            SubCommand::Serve(args) => serve::run(args).await?,
            SubCommand::Certs(args) => certs::run(args).await?,
            SubCommand::Download(args) => download::run(args).await?,
            SubCommand::Seed(args) => seed::run(args).await?,
            SubCommand::Torrent { torrent } => match torrent {
                Torrent::Inspect { torrent } => inspect(torrent, cli.verbose)?,
                Torrent::Validate { torrent } => validate(torrent)?,
                Torrent::Create(args) => create(args)?,
            },
        }
    }

    Ok(())
}
