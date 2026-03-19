use std::{error::Error, io};

use bqti::{
    cli::{Cli, SubCommand, Torrent},
    network::{
        peer::Peer,
        session::{Message, PeerSession},
    },
    torrent::{create, inspect, validate},
};
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .compact()
        .with_writer(io::stderr)
        .with_env_filter(filter)
        .init();

    let cli = Cli::parse();

    if let Some(subcommand) = cli.subcommand {
        match subcommand {
            SubCommand::Serve { addr, to } => {
                let my_self = Peer::new("peer-a", &addr)?;
                let other_self = Peer::new("peer-b", &to)?;

                let mut session = PeerSession::new(my_self);
                let mut message_recv = session.listening(other_self).await?;

                while let Some(message) = message_recv.recv().await {
                    match message {
                        Message::KeepAlive => info!("keep alive"),
                        Message::DHT(payload) => info!("dht: {}", hex::encode(payload)),
                        Message::PEX(payload) => info!("pex: {}", hex::encode(payload)),
                        Message::Standard(payload) => info!("bit: {}", hex::encode(payload)),
                    }
                }

                Ok(())
            }
            SubCommand::Torrent { torrent } => match torrent {
                Torrent::Inspect { torrent } => inspect(torrent, cli.verbose),
                Torrent::Validate { torrent } => validate(torrent),
                Torrent::Create(args) => create(args),
            },
        }?;
    }

    Ok(())
}
