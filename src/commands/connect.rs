use crate::{
    network::{ConnectionManager, ManagerOptions, Message, Packet, Peer, QuicEndpointBuilder},
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

    let ca_root = utils::certs::make_ca_root().await?;
    let leaf_quic = ca_root.leaf("QUIC", false)?;

    let endpoint_config = QuicEndpointBuilder::new(
        my_self.address,
        vec![leaf_quic.cert_der(), ca_root.cert_der()],
        leaf_quic.key_der(),
    );

    let (manager, mut stream_rx) =
        ConnectionManager::new(endpoint_config, ManagerOptions::default())?;

    manager.connect(&other_self).await?;

    tokio::spawn(async move {
        while let Some(Packet(message, _)) = stream_rx.recv().await {
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
