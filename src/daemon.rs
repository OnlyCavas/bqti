use std::sync::Arc;

use bqti_ipc::{Request, Response, Socket};

use crate::{
    Bqti,
    ipc::server::IpcServer,
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
    let (ipc, cmd_rx) = IpcServer::bind().await?;

    let mut bit_torrent = Bqti::new(Arc::new(manager), leaf_kademlia)?;
    bit_torrent.serve_forever(stream_rx, cmd_rx).await?;

    drop(ipc);

    Ok(())
}

pub async fn handle_download(torrent: String, _output: String) -> anyhow::Result<()> {
    let mut socket = Socket::connect().await?;

    let response = socket
        .send(Request::AddDownload { source: torrent })
        .await?
        .map_err(|e| anyhow::anyhow!(e))?;

    match response {
        Response::TorrentAdded { info_hash } => {
            info!("torrent added: {info_hash}");

            println!("{info_hash}");
        }
        unexpected => {
            anyhow::bail!("unexpected response from daemon: {unexpected:?}");
        }
    }

    Ok(())
}

pub async fn handle_seed(torrent: String, _output: String) -> anyhow::Result<()> {
    let mut socket = Socket::connect().await?;

    let response = socket
        .send(Request::AddDownload { source: torrent })
        .await?
        .map_err(|e| anyhow::anyhow!(e))?;

    match response {
        Response::TorrentAdded { info_hash } => {
            info!("torrent added: {info_hash}");

            println!("{info_hash}");
        }
        unexpected => {
            anyhow::bail!("unexpected response from daemon: {unexpected:?}");
        }
    }

    Ok(())
}
