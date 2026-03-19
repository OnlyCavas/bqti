use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use quinn::{ClientConfig, Connection, RecvStream, SendStream};
use quinn::{Endpoint, ServerConfig};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::bit_torrent::network::peer::Peer;

pub type StreamPair = (SendStream, RecvStream);

#[async_trait::async_trait]
pub trait Connector {
    async fn connect(
        &self,
        peer: Peer,
    ) -> Result<(Connection, mpsc::Receiver<StreamPair>), Box<dyn std::error::Error>>;
}

pub struct QuicManager {
    endpoint: Endpoint,
}

impl QuicManager {
    // FIX is missing create correctly the root CA to establish connection between two peers
    // FIX maybe remove one tokio::spawn at connection.rs
    pub fn new(addr: SocketAddr) -> Result<Self, Box<dyn std::error::Error>> {
        let crypto_provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());

        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".into()])?;

        let cert_der = CertificateDer::from(cert.der().to_vec());

        let key_bytes = signing_key.serialize_der();
        let priv_key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_bytes));

        let mut server_crypto =
            rustls::ServerConfig::builder_with_provider(crypto_provider.clone())
                .with_safe_default_protocol_versions()?
                .with_no_client_auth()
                .with_single_cert(vec![cert_der], priv_key)?;

        server_crypto.alpn_protocols = vec![b"bittorrent-quic".to_vec()];
        let server_config = ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)?,
        ));

        let mut client_crypto = rustls::ClientConfig::builder_with_provider(crypto_provider)
            .with_safe_default_protocol_versions()?
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();

        client_crypto.alpn_protocols = vec![b"bittorrent-quic".to_vec()];
        let client_config = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)?,
        ));

        let mut endpoint = Endpoint::server(server_config, addr)?;
        endpoint.set_default_client_config(client_config);

        Ok(Self { endpoint })
    }
}

#[async_trait::async_trait]
impl Connector for QuicManager {
    async fn connect(
        &self,
        peer: Peer,
    ) -> Result<(Connection, mpsc::Receiver<StreamPair>), Box<dyn std::error::Error>> {
        let endpoint = self.endpoint.connect(peer.address, &peer.id)?;

        info!("listening");
        let connection = timeout(Duration::from_secs(5), endpoint).await??;

        info!("nevermind");
        let (tx, rx) = mpsc::channel::<StreamPair>(32);

        let handle = connection.clone();
        tokio::spawn(async move {
            loop {
                match handle.accept_bi().await {
                    Ok(streams) => {
                        if tx.send(streams).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok((connection, rx))
    }
}
