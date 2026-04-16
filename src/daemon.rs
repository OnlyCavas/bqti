use std::sync::Arc;

use crate::{
    Bqti,
    network::{ConnectionManager, ManagerOptions, Peer, QuicEndpointBuilder},
    utils,
};

pub async fn handle_serve(addr: String) -> anyhow::Result<()> {
    let my_self = Peer::new("localhost", &addr)?;
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
