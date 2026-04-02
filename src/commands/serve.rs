use std::sync::Arc;

use anyhow::Result;
use clap::Parser;

use crate::{
    Bqti,
    network::{ConnectionManager, ManagerOptions, Peer, QuicEndpointBuilder},
    utils::{self},
};

#[derive(Parser, Debug)]
pub struct ServeArgs {
    #[arg(value_parser = parse_addr)]
    addr: String,
}

fn parse_addr(addr: &str) -> Result<String, String> {
    if addr.starts_with(":") {
        return Ok(format!("127.0.0.1{}", addr));
    }

    if addr.starts_with("localhost") {
        return Ok(addr.replace("localhost", "127.0.0.1"));
    }

    Ok(addr.to_string())
}

pub async fn run(args: ServeArgs) -> Result<()> {
    let my_self = Peer::new("localhost", &args.addr)?;
    let ca_root = utils::certs::make_ca_root().await?;

    let leaf_quic = ca_root.leaf("localhost", false)?;
    let leaf_kademlia = ca_root.leaf("kademlia", false)?;

    let endpoint_config = QuicEndpointBuilder::new(
        my_self.address,
        vec![leaf_quic.cert_der(), ca_root.cert_der()],
        leaf_quic.key_der(),
    );

    let (manager, stream_rx) = ConnectionManager::new(endpoint_config, ManagerOptions::default())?;

    let bit_torrent = Bqti::new(Arc::new(manager), leaf_kademlia)?;
    bit_torrent.serve_forever(stream_rx).await?;

    Ok(())
}
