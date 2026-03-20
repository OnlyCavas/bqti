use std::{net::SocketAddr, sync::Arc, time::Duration};

use quinn::{
    IdleTimeout,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
};
use rustls::crypto::CryptoProvider;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use anyhow::Result;

pub struct QuicEndpointBuilder {
    addr: SocketAddr,
    transport_config: quinn::TransportConfig,
    crypto_provider: Arc<CryptoProvider>,
    certs: Vec<CertificateDer<'static>>,
    priv_key: PrivateKeyDer<'static>,
}

impl QuicEndpointBuilder {
    pub fn new(
        addr: SocketAddr,
        certs: Vec<CertificateDer<'static>>,
        priv_key: PrivateKeyDer<'static>,
    ) -> Self {
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.keep_alive_interval(Some(Duration::from_secs(10)));
        transport_config.max_idle_timeout(Some(IdleTimeout::from(quinn::VarInt::from_u32(60_000))));

        Self {
            addr,
            transport_config,
            crypto_provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
            certs,
            priv_key,
        }
    }

    pub fn transport(mut self, config: quinn::TransportConfig) -> Self {
        self.transport_config = config;
        self
    }

    fn server_crypto(&self) -> Result<rustls::ServerConfig> {
        let mut server_crypto =
            rustls::ServerConfig::builder_with_provider(self.crypto_provider.clone())
                .with_safe_default_protocol_versions()?
                .with_no_client_auth()
                .with_single_cert(self.certs.clone(), self.priv_key.clone_key())?;

        server_crypto.alpn_protocols = vec![b"bittorrent-quic".to_vec()];

        Ok(server_crypto)
    }

    fn client_crypto(&self) -> Result<rustls::ClientConfig> {
        let mut root_store = rustls::RootCertStore::empty();
        root_store.add_parsable_certificates(self.certs.clone());

        let mut client_crypto =
            rustls::ClientConfig::builder_with_provider(self.crypto_provider.clone())
                .with_safe_default_protocol_versions()?
                .with_root_certificates(root_store)
                .with_no_client_auth();

        client_crypto.alpn_protocols = vec![b"bittorrent-quic".to_vec()];

        Ok(client_crypto)
    }

    pub fn build(self) -> Result<quinn::Endpoint> {
        let server_crypto = self.server_crypto()?;
        let client_crypto = self.client_crypto()?;

        let quic_server_config = QuicServerConfig::try_from(server_crypto)?;
        let quic_client_config = QuicClientConfig::try_from(client_crypto)?;

        let transport = Arc::new(self.transport_config);

        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_server_config));
        server_config.transport_config(transport.clone());

        let mut client_config = quinn::ClientConfig::new(Arc::new(quic_client_config));
        client_config.transport_config(transport);

        let mut endpoint = quinn::Endpoint::server(server_config, self.addr)?;
        endpoint.set_default_client_config(client_config);

        Ok(endpoint)
    }
}
