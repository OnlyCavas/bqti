use crate::{
    network::{ConnectionManager, LeafCert, ManagerOptions, Message, Peer, QuicEndpointBuilder},
    utils,
};

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
pub struct ConnectArgs {
    #[arg(short, long)]
    addr: String,

    #[arg(short, long)]
    to: String,
}

pub async fn run(args: ConnectArgs) -> Result<()> {
    let my_self = Peer::new("peer-a", &args.addr)?;
    let other_self = Peer::new("peer-b", &args.to)?;

    let root_cert = utils::certs::load_or_generate_root_ca().await?;
    let (root_cert_der, _) = root_cert.cert.der();

    let leaf_cert = LeafCert::generate(vec![other_self.id.clone()], &root_cert)?;
    let (leaf_cert_der, leaf_priv_key) = leaf_cert.cert.der();

    let endpoint_config = QuicEndpointBuilder::new(
        my_self.address,
        vec![leaf_cert_der, root_cert_der],
        leaf_priv_key,
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
