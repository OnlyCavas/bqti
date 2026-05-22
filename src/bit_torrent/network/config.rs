use std::{net::SocketAddr, sync::Arc, time::Duration};

use quinn::{
    IdleTimeout,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
};
use rustls::{crypto::CryptoProvider, sign::CertifiedKey};
use rustls_pki_types::CertificateDer;

use anyhow::Result;

use crate::certs::{NoVerifier, SingleCertResolver};

pub struct QuicEndpointBuilder {
    addr: SocketAddr,
    transport_config: quinn::TransportConfig,
    crypto_provider: Arc<CryptoProvider>,
    certs: Vec<CertificateDer<'static>>,
    certified_key: Arc<CertifiedKey>,
    skip_cert_verify: bool,
}

impl QuicEndpointBuilder {
    pub fn new(
        addr: SocketAddr,
        certs: Vec<CertificateDer<'static>>,
        certified_key: Arc<CertifiedKey>,
    ) -> Self {
        let mut transport_config = quinn::TransportConfig::default();

        transport_config.keep_alive_interval(Some(Duration::from_secs(1)));
        transport_config.max_idle_timeout(Some(IdleTimeout::from(quinn::VarInt::from_u32(10_000))));
        transport_config.enable_segmentation_offload(false);

        Self {
            addr,
            transport_config,
            crypto_provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
            certs,
            certified_key,
            skip_cert_verify: false,
        }
    }

    pub fn transport(mut self, config: quinn::TransportConfig) -> Self {
        self.transport_config = config;
        self
    }

    fn server_crypto(&self) -> anyhow::Result<rustls::ServerConfig> {
        let mut server_crypto =
            rustls::ServerConfig::builder_with_provider(self.crypto_provider.clone())
                .with_safe_default_protocol_versions()?
                .with_no_client_auth()
                .with_cert_resolver(Arc::new(SingleCertResolver(self.certified_key.clone())));

        server_crypto.alpn_protocols = vec![b"bittorrent-quic".to_vec()];
        Ok(server_crypto)
    }

    fn client_crypto(&self) -> Result<rustls::ClientConfig> {
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

            rustls::ClientConfig::builder_with_provider(self.crypto_provider.clone())
                .with_safe_default_protocol_versions()?
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };

        client_crypto.alpn_protocols = vec![b"bittorrent-quic".to_vec()];
        Ok(client_crypto)
    }

    pub fn dangerous_no_cert_verify(mut self) -> Self {
        self.skip_cert_verify = true;
        self
    }

    pub fn build(self) -> anyhow::Result<quinn::Endpoint> {
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
