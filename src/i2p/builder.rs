use std::{sync::Arc, time::Duration};

use quinn::{Endpoint, EndpointConfig, ServerConfig};
use rustls::crypto::CryptoProvider;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use crate::{
    i2p::{I2pDatagramSocket, SamSession, dest_map::DestMap},
    network::NoVerifier,
};

type I2pEndpoint = (quinn::Endpoint, Arc<I2pDatagramSocket>);

pub struct I2pEndpointBuilder {
    crypto_provider: Arc<CryptoProvider>,
    certs: Vec<CertificateDer<'static>>,
    priv_key: PrivateKeyDer<'static>,

    skip_cert_verify: bool,
}

impl I2pEndpointBuilder {
    pub fn new(certs: Vec<CertificateDer<'static>>, priv_key: PrivateKeyDer<'static>) -> Self {
        Self {
            crypto_provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
            certs,
            priv_key,
            skip_cert_verify: false,
        }
    }

    pub fn dangerous_no_cert_verify(mut self) -> Self {
        self.skip_cert_verify = true;
        self
    }

    fn server_crypto(&self) -> anyhow::Result<rustls::ServerConfig> {
        let mut server_crypto =
            rustls::ServerConfig::builder_with_provider(self.crypto_provider.clone())
                .with_safe_default_protocol_versions()?
                .with_no_client_auth()
                .with_single_cert(self.certs.clone(), self.priv_key.clone_key())?;

        server_crypto.alpn_protocols = vec![b"bittorrent-quic".to_vec()];

        Ok(server_crypto)
    }

    fn transport_config() -> Arc<quinn::TransportConfig> {
        let mut t = quinn::TransportConfig::default();

        t.keep_alive_interval(Some(Duration::from_secs(5)));
        t.max_idle_timeout(Some(quinn::IdleTimeout::from(quinn::VarInt::from_u32(
            60_000,
        ))));
        t.enable_segmentation_offload(false);

        Arc::new(t)
    }

    fn create_endpoint(
        session: SamSession,
        server_config: ServerConfig,
    ) -> anyhow::Result<I2pEndpoint> {
        let dest_map = Arc::new(DestMap::new());
        let socket = Arc::new(I2pDatagramSocket::new(Arc::new(session), dest_map));

        let endpoint = Endpoint::new_with_abstract_socket(
            EndpointConfig::default(),
            Some(server_config),
            socket.clone(),
            Arc::new(quinn::TokioRuntime),
        )?;

        Ok((endpoint, socket))
    }

    fn client_crypto(&self) -> anyhow::Result<rustls::ClientConfig> {
        let builder = rustls::ClientConfig::builder_with_provider(self.crypto_provider.clone())
            .with_safe_default_protocol_versions()?;

        let mut client_crypto = if self.skip_cert_verify {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerifier::new(
                    self.crypto_provider.clone(),
                )))
                .with_no_client_auth()
        } else {
            let mut root_store = rustls::RootCertStore::empty();
            root_store.add_parsable_certificates(self.certs.clone());

            builder
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };

        client_crypto.alpn_protocols = vec![b"bittorrent-quic".to_vec()];

        Ok(client_crypto)
    }

    fn get_session_id() -> String {
        format!("bqti-{:016x}", rand::random::<u64>())
    }

    pub async fn build(self) -> anyhow::Result<I2pEndpoint> {
        let server_crypto = self.server_crypto()?;
        let client_crypto = self.client_crypto()?;

        let server_tls = quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)?;
        let client_tls = quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)?;

        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(server_tls));
        server_config.transport_config(Self::transport_config());

        let mut client_config = quinn::ClientConfig::new(Arc::new(client_tls));
        client_config.transport_config(Self::transport_config());

        let session = SamSession::new(&Self::get_session_id()).await?;

        info!("─── I2P ENDPOINT ───");
        info!("B32 Address:");
        info!("  > {}", session.b32_addr);
        info!("");
        info!("Full Destination (B64):");

        for chunk in session.destination.as_bytes().chunks(64) {
            if let Ok(s) = std::str::from_utf8(chunk) {
                info!("  {}", s);
            }
        }
        info!("───────────────────");

        let (mut endpoint, socket) = Self::create_endpoint(session, server_config)?;
        endpoint.set_default_client_config(client_config);

        Ok((endpoint, socket))
    }
}
