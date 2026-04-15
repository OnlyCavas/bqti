use std::io;

use anyhow::Result;
use bqti::{
    cli::{Cli, SubCommand},
    daemon, ipc, standalone,
};

use clap::{CommandFactory, Parser};
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

    let Some(subcommand) = cli.subcommand else {
        Cli::command().print_help()?;
        return Ok(());
    };

    match subcommand {
        SubCommand::Daemon(daemon) => ipc::handle_client(daemon).await,
        SubCommand::Certs { kind, output } => standalone::handle_certs(kind, output).await,
        SubCommand::Torrent { torrent } => standalone::handle_torrent(torrent, cli.verbose),
        SubCommand::Serve { addr } => daemon::handle_serve(addr).await,
    }?;

    Ok(())
}
