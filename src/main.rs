use bqti::{
    BQTIError,
    cli::{Cli, SubCommand, Torrent},
    torrent::{inspect, validate},
};
use clap::Parser;

fn main() -> Result<(), BQTIError> {
    let cli = Cli::parse();

    if let Some(subcommand) = cli.subcommand {
        match subcommand {
            SubCommand::Torrent { torrent } => match torrent {
                Torrent::Inspect { torrent } => inspect(torrent, cli.verbose)?,
                Torrent::Validate { torrent } => validate(torrent)?,
            },
        };
    }

    Ok(())
}
