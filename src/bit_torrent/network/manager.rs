use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use ::futures::future::join_all;
use anyhow::Result;
use quinn::{ConnectError, ConnectionError};
use thiserror::Error;
use tokio::{
    sync::{RwLock, mpsc},
    task::JoinSet,
    time::timeout,
};

use tokio_util::sync::CancellationToken;

#[derive(Debug, Error)]
pub enum ConnectionManagerError {
    #[error(transparent)]
    ConnectError(#[from] ConnectError),

    #[error(transparent)]
    QuicConnectionError(#[from] ConnectionError),

    #[error(transparent)]
    ConnectionError(#[from] connection::ConnectionError),

    #[error("failed to establish a connection, with {0}")]
    EstablishError(String),

    #[error("failed to fetch local ip address")]
    LocalIpError(),
}

use crate::network::{
    config::QuicEndpointBuilder,
    connection::{self, Connection, OnDisconnect},
    message::{Message, Packet},
    peer::Peer,
};

const CHANNEL_BUFFER_SIZE: usize = 1024;

#[derive(Clone)]
pub struct ManagerOptions {
    pub max_connections: usize,
    pub handshake_timeout: Duration,
}

impl ManagerOptions {}

impl Default for ManagerOptions {
    fn default() -> Self {
        Self {
            max_connections: 50,
            handshake_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Clone)]
pub struct ConnectionManager {
    endpoint: quinn::Endpoint,
    connections: Arc<RwLock<HashMap<String, Connection>>>,
    options: ManagerOptions,
    pub message_tx: mpsc::Sender<Packet>,
}

impl ConnectionManager {
    pub fn new(
        tls_endpoint: QuicEndpointBuilder,
        options: ManagerOptions,
    ) -> Result<(Self, mpsc::Receiver<Packet>)> {
        let (tx, stream_rx) = mpsc::channel::<Packet>(CHANNEL_BUFFER_SIZE);

        let endpoint = tls_endpoint.build()?;

        let manager = Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            endpoint,
            message_tx: tx,
            options,
        };

        Ok((manager, stream_rx))
    }

    pub fn get_local_ip(self) -> Result<SocketAddr, ConnectionManagerError> {
        self.endpoint
            .local_addr()
            .map_err(|_| ConnectionManagerError::LocalIpError())
    }

    fn on_disconnect_handler(&self) -> OnDisconnect {
        let conns_arc = self.connections.clone();

        Arc::new(move |id| {
            let conns = conns_arc.clone();
            tokio::spawn(async move {
                conns.write().await.remove(&id);
                info!("peer disconnected: {}", id);
            });
        })
    }

    async fn add_peer(
        connections: &Arc<RwLock<HashMap<String, Connection>>>,
        peer_id: String,
        connection: Connection,
    ) {
        let mut conns = connections.write().await;

        if conns.contains_key(&peer_id) {
            info!("peer already connected: {}", peer_id);
            return;
        }

        conns.insert(peer_id, connection);
    }

    pub async fn start_listening(&self, cancel: CancellationToken) {
        let mut join_handle = JoinSet::new();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    join_handle.shutdown().await;
                    break;
                },
                Some(incoming) = async {  self.endpoint.accept().await} => {
                    let handshake_timeout = self.options.handshake_timeout;

                    if self.endpoint.open_connections() >= self.options.max_connections {
                        info!("max connections reached: {}", self.options.max_connections);
                        incoming.refuse();
                        continue;
                    }

                    let on_disconnect = self.on_disconnect_handler();
                    let connections = self.connections.clone();
                    let dispatcher = self.message_tx.clone();

                    join_handle.spawn(async move {
                        let conn_result = timeout(handshake_timeout, incoming).await;

                        let Ok(Ok(conn)) = conn_result else {
                            warn!(
                                "handshake failed, timout of {} seconds exceeded",
                                handshake_timeout.as_secs()
                            );
                            return;
                        };

                        let peer_id = conn.remote_address().to_string();

                        let Ok(connection) =
                            Connection::spawn(peer_id.clone(), conn, dispatcher, on_disconnect).await
                        else {
                            error!("failed to listen to new connections");
                            return;
                        };

                        Self::add_peer(&connections, peer_id, connection).await;
                    });
                }
            }
        }
    }

    pub async fn connect(&self, peer: &Peer) -> Result<(), ConnectionManagerError> {
        let peer_id = peer.address.to_string();

        {
            let conns = self.connections.read().await;

            if let Some(_conn) = conns.get(&peer_id) {
                return Ok(());
            }
        }

        // FIX &peer.id
        let connecting = self.endpoint.connect(peer.address, "localhost")?.await?;

        let connection = Connection::spawn(
            peer_id.clone(),
            connecting,
            self.message_tx.clone(),
            self.on_disconnect_handler(),
        )
        .await?;

        Self::add_peer(&self.connections, peer_id, connection).await;

        Ok(())
    }

    pub async fn send(
        &self,
        peer: &Peer,
        msg: impl Into<Message>,
    ) -> Result<(), ConnectionManagerError> {
        let conns = self.connections.read().await;
        let peer_id = peer.address.to_string();

        let Some(conn) = conns.get(&peer_id) else {
            error!("failed to send message");
            return Err(ConnectionManagerError::EstablishError(peer_id));
        };

        conn.send_message(msg.into()).await?;

        Ok(())
    }

    pub async fn shutdown(&self) {
        let mut conns = self.connections.write().await;

        let tasks: Vec<_> = conns
            .drain()
            .map(|(_, connection)| connection.close())
            .collect();

        join_all(tasks).await;
    }
}
