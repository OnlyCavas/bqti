use std::{error::Error, io};

use bqti::{
    cli::{Cli, SubCommand, Torrent},
    network::{
        certs::{LeafCert, RootCA},
        config::QuicEndpointBuilder,
        manager::{ConnectionManager, ManagerOptions},
        message::Message,
        peer::Peer,
    },
    torrent::{create, inspect, validate},
};
use clap::Parser;
use tokio::fs;
use tracing::info;
use tracing_subscriber::EnvFilter;

async fn load_and_gen(
    ca_root_path: &str,
    pk_root_path: &str,
    dest: String,
) -> Result<(RootCA, LeafCert), Box<dyn std::error::Error>> {
    let ca = RootCA::load_or_generate(ca_root_path, pk_root_path).await?;
    let peer_cert = LeafCert::generate(vec![dest.clone()], &ca)?;

    Ok((ca, peer_cert))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let ca_root_path = "./resources/certs/ca.der";
    let pk_root_path = "./resources/certs/pk.der";

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .compact()
        .with_writer(io::stderr)
        .with_env_filter(filter)
        .init();

    let cli = Cli::parse();

    if let Some(subcommand) = cli.subcommand {
        match subcommand {
            SubCommand::Gen => {
                let ca = RootCA::generate()?;
                let (cert_bytes, pk_bytes) = ca.cert.der();

                fs::write(ca_root_path, cert_bytes).await?;
                fs::write(pk_root_path, pk_bytes.secret_der().to_vec()).await?;

                Ok(())
            }
            SubCommand::Serve { addr, to } => {
                let my_self = Peer::new("peer-a", &addr)?;
                let other_self = Peer::new("peer-b", &to)?;

                // load certs
                let (root_ca, leaf_ct) =
                    load_and_gen(ca_root_path, pk_root_path, other_self.id).await?;

                let ca_cert_der = root_ca.cert.der();
                let peer_cert_der = leaf_ct.cert.der();
                let peer_priv_key = peer_cert_der.1;

                let endpoint_config = QuicEndpointBuilder::new(
                    my_self.address,
                    vec![peer_cert_der.0, ca_cert_der.0],
                    peer_priv_key,
                );

                let (manager, mut stream_rx) =
                    ConnectionManager::new(endpoint_config, ManagerOptions::default())?;

                let manager_tx = manager.clone();
                tokio::spawn(async move {
                    manager_tx.start_listening().await;
                });

                while let Some(message) = stream_rx.recv().await {
                    match message {
                        Message::KeepAlive => info!("keep alive"),
                        Message::DHT(payload) => info!("dht: {}", hex::encode(payload)),
                        Message::PEX(payload) => info!("pex: {}", hex::encode(payload)),
                        Message::Standard(payload) => info!("bit: {}", hex::encode(payload)),
                    }
                }

                tokio::signal::ctrl_c().await?;
                manager.shutdown().await;

                Ok(())
            }
            SubCommand::Connect { addr, to } => {
                let my_self = Peer::new("peer-a", &addr)?;
                let other_self = Peer::new("peer-b", &to)?;

                // load certs
                let (root_ca, leaf_ct) =
                    load_and_gen(ca_root_path, pk_root_path, other_self.id.clone()).await?;

                let ca_cert_der = root_ca.cert.der();
                let peer_cert_der = leaf_ct.cert.der();
                let peer_priv_key = peer_cert_der.1;

                let endpoint_config = QuicEndpointBuilder::new(
                    my_self.address,
                    vec![peer_cert_der.0, ca_cert_der.0],
                    peer_priv_key,
                );

                let (manager, mut stream_rx) =
                    ConnectionManager::new(endpoint_config, ManagerOptions::default())?;

                manager.connect(&other_self).await?;

                tokio::spawn(async move {
                    while let Some(message) = stream_rx.recv().await {
                        match message {
                            Message::KeepAlive => info!("keep alive"),
                            Message::DHT(payload) => info!("dht: {}", hex::encode(payload)),
                            Message::PEX(payload) => info!("pex: {}", hex::encode(payload)),
                            Message::Standard(payload) => info!("bit: {}", hex::encode(payload)),
                        }
                    }
                });

                manager.send(&other_self, Message::KeepAlive).await?;
                tokio::signal::ctrl_c().await?;
                manager.shutdown().await;

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
