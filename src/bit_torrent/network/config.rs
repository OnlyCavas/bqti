use std::{net::SocketAddr, sync::Arc, time::Duration};

use quinn::{
    IdleTimeout,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::CryptoProvider,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use anyhow::Result;

pub struct QuicEndpointBuilder {
    addr: SocketAddr,
    transport_config: quinn::TransportConfig,
    crypto_provider: Arc<CryptoProvider>,
    certs: Vec<CertificateDer<'static>>,
    priv_key: PrivateKeyDer<'static>,
    skip_cert_verify: bool,
}

#[derive(Debug)]
struct NoVerifier {
    crypto_provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls_pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls_pki_types::UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.crypto_provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

impl QuicEndpointBuilder {
    pub fn new(
        addr: SocketAddr,
        certs: Vec<CertificateDer<'static>>,
        priv_key: PrivateKeyDer<'static>,
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
            priv_key,
            skip_cert_verify: false,
        }
    }

    pub fn dangerous_no_cert_verify(mut self) -> Self {
        self.skip_cert_verify = true;
        self
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
        let builder = rustls::ClientConfig::builder_with_provider(self.crypto_provider.clone())
            .with_safe_default_protocol_versions()?;

        let mut client_crypto = if self.skip_cert_verify {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerifier {
                    crypto_provider: self.crypto_provider.clone(),
                }))
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
