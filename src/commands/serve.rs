use std::sync::Arc;

use anyhow::Result;
use clap::Parser;

use crate::{
    Bqti,
    network::{ConnectionManager, LeafCert, ManagerOptions, Peer, QuicEndpointBuilder},
    utils::{self},
};

#[derive(Parser, Debug)]
pub struct ServeArgs {
    #[arg(short, long)]
    addr: String,
}

pub async fn run(args: ServeArgs) -> Result<()> {
    let my_self = Peer::new("localhost", &args.addr)?;

    let root_cert = utils::certs::load_or_generate_root_ca().await?;
    let (root_cert_der, _) = root_cert.cert.der();

    let leaf_cert = LeafCert::generate(vec![my_self.id], &root_cert)?;
    let (leaf_cert_der, leaf_priv_key) = leaf_cert.cert.der();

    let endpoint_config = QuicEndpointBuilder::new(
        my_self.address,
        vec![leaf_cert_der, root_cert_der],
        leaf_priv_key,
    );

    let (manager, stream_rx) = ConnectionManager::new(endpoint_config, ManagerOptions::default())?;

    let bit_torrent = Bqti::new(&args.addr, Arc::new(manager))?;
    bit_torrent.serve_forever(stream_rx).await?;

    Ok(())
}
