use anyhow::Result;
use clap::Parser;
use tokio_util::sync::CancellationToken;

use crate::{
    network::{ConnectionManager, LeafCert, ManagerOptions, Message, Peer, QuicEndpointBuilder},
    utils,
};

#[derive(Parser, Debug)]
pub struct ServeArgs {
    #[arg(short, long)]
    addr: String,

    #[arg(short, long)]
    to: String,
}

pub async fn run(args: ServeArgs) -> Result<()> {
    let my_self = Peer::new("peer-a", &args.addr)?;
    let other_self = Peer::new("peer-b", &args.to)?;

    let root_cert = utils::certs::load_or_generate_root_ca().await?;
    let (root_cert_der, _) = root_cert.cert.der();

    let leaf_cert = LeafCert::generate(vec![other_self.id], &root_cert)?;
    let (leaf_cert_der, leaf_priv_key) = leaf_cert.cert.der();

    let endpoint_config = QuicEndpointBuilder::new(
        my_self.address,
        vec![leaf_cert_der, root_cert_der],
        leaf_priv_key,
    );

    let (manager, mut stream_rx) =
        ConnectionManager::new(endpoint_config, ManagerOptions::default())?;
    let cancelation_token = CancellationToken::new();

    let manager_tx = manager.clone();
    let cancel_tx = cancelation_token.clone();

    tokio::spawn(async move {
        manager_tx.start_listening(cancel_tx).await;
    });

    loop {
        tokio::select! {
            Some(message) = stream_rx.recv() => {
                match message {
                    Message::KeepAlive => info!("keep alive"),
                    Message::DHT(payload) => info!("dht: {}", hex::encode(payload)),
                    Message::PEX(payload) => info!("pex: {}", hex::encode(payload)),
                    Message::Standard(payload) => info!("bit: {}", hex::encode(payload)),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    cancelation_token.cancel();
    manager.shutdown().await;

    Ok(())
}

