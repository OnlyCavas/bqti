use anyhow::Context;

use crate::{
    Bqti, EndpointBuilder,
    certs::{ActiveKeyIdentity, KeyIdentity},
    i2p,
    network::{ConnectionManager, ManagerOptions, Peer, QuicEndpointBuilder},
};

pub async fn handle_serve(addr: String, no_cert: bool, i2p: bool) -> anyhow::Result<()> {
    let my_self = Peer::new("localhost", &addr)?;

    let ca_root: ActiveKeyIdentity = {
        #[cfg(feature = "tee")]
        {
            use crate::certs::TeeKeyIdentity;
            TeeKeyIdentity::new()?
        }
        #[cfg(not(feature = "tee"))]
        {
            use crate::utils;
            utils::certs::make_ca_root().await?
        }
    };

    let leaf_quic = ca_root.leaf("localhost", false)?;
    let leaf_kademlia = ca_root.leaf("kademlia", false)?;

    let endpoint_config: EndpointBuilder = match i2p {
        true => {
            let endpoint_config = i2p::I2pEndpointBuilder::new(
                vec![leaf_quic.cert_der(), ca_root.cert_der()],
                leaf_quic.certified_key(),
            );

            EndpointBuilder::I2p(endpoint_config)
        }
        false => {
            let endpoint_config = QuicEndpointBuilder::new(
                my_self.address,
                vec![leaf_quic.cert_der(), ca_root.cert_der()],
                leaf_quic.certified_key(),
            );

            EndpointBuilder::Quic(endpoint_config)
        }
    };

    let endpoint = match no_cert {
        true => {
            warn!("TLS certificate verification disabled — do not use in production");
            endpoint_config.dangerous_no_cert_verify().build()
        }
        false => endpoint_config.build(),
    }
    .await
    .context("failed to build quic configuration")?;

    let (manager, stream_rx) = ConnectionManager::new(endpoint, ManagerOptions::default())?;

    let bit_torrent = Bqti::new(manager, leaf_kademlia)?;
    bit_torrent.serve_forever(stream_rx).await?;

    Ok(())
}
