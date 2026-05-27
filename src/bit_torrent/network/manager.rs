use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::Result;
use quinn::{ConnectError, ConnectionError};
use thiserror::Error;
use tokio::{
    sync::{RwLock, broadcast, mpsc},
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

    #[error("can't establish a self connection")]
    SelfConnectionError(),

    #[error("failed to fetch local ip address")]
    LocalIpError(),
}

use crate::network::{
    connection::{self, BidirectionalStream, Connection, ControlStream, OnDisconnect},
    endpoint::NetworkEndpoint,
    message::{Message, Packet},
    peer::Peer,
};

const CHANNEL_BUFFER_SIZE: usize = 1024;
const DISCONNECT_SOCKETS_SIZE: usize = 64;

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
            handshake_timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Clone)]
pub struct ConnectionManager {
    pub endpoint: NetworkEndpoint,
    connections: Arc<RwLock<HashMap<SocketAddr, Arc<Connection>>>>,
    connecting: Arc<RwLock<HashSet<SocketAddr>>>,
    next_id: Arc<AtomicU64>,
    options: ManagerOptions,
    pub message_tx: mpsc::Sender<Packet>,
    disconnect_tx: broadcast::Sender<SocketAddr>,
}

impl ConnectionManager {
    pub fn new(
        endpoint: NetworkEndpoint,
        options: ManagerOptions,
    ) -> Result<(Arc<Self>, mpsc::Receiver<Packet>)> {
        let (tx, stream_rx) = mpsc::channel::<Packet>(CHANNEL_BUFFER_SIZE);
        let (disconnect_tx, _) = broadcast::channel::<SocketAddr>(DISCONNECT_SOCKETS_SIZE);

        let manager = Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            connecting: Arc::new(RwLock::new(HashSet::new())),
            endpoint,
            message_tx: tx,
            options,
            disconnect_tx,
            next_id: Arc::new(AtomicU64::new(0)),
        };

        Ok((Arc::new(manager), stream_rx))
    }

    pub async fn disconnect(&self, addr: &SocketAddr) {
        let mut conns = self.connections.write().await;

        let Some(conn) = conns.remove(&addr) else {
            return;
        };

        conn.close();
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn subscribe_disconnects(&self) -> broadcast::Receiver<SocketAddr> {
        self.disconnect_tx.subscribe()
    }

    pub fn get_local_ip(&self) -> Result<SocketAddr, ConnectionManagerError> {
        let addr = self
            .endpoint
            .local_addr()
            .map_err(|_| ConnectionManagerError::LocalIpError())?;

        if !addr.ip().is_unspecified() {
            return Ok(addr);
        }

        let ip = {
            let socket = std::net::UdpSocket::bind("0.0.0.0:0")
                .and_then(|s| {
                    s.connect("8.8.8.8:80")?;
                    Ok(s)
                })
                .map_err(|_| ConnectionManagerError::LocalIpError())?;

            socket
                .local_addr()
                .map_err(|_| ConnectionManagerError::LocalIpError())?
                .ip()
        };

        Ok(SocketAddr::new(ip, addr.port()))
    }

    pub async fn get_connection(&self, socket_addr: &SocketAddr) -> Option<Arc<Connection>> {
        let connection = self.connections.read().await;
        connection.get(socket_addr).cloned()
    }

    fn on_disconnect_handler(&self, connection_id: u64) -> OnDisconnect {
        let weak_connections = Arc::downgrade(&self.connections);
        let disconnect_tx = self.disconnect_tx.clone();

        Arc::new(move |addr| {
            let connections = match weak_connections.upgrade() {
                Some(s) => s,
                None => return,
            };

            let disconnect_tx = disconnect_tx.clone();

            tokio::spawn(async move {
                let mut connections = connections.write().await;

                if connections.get(&addr).map(|c| c.id) != Some(connection_id) {
                    return;
                }

                debug!("peer disconnected: {}", addr);

                connections.remove(&addr);
                drop(connections);
                let _ = disconnect_tx.send(addr);
            });
        })
    }

    async fn append_connection(
        &self,
        connection: Arc<Connection>,
        peer_addr: &SocketAddr,
    ) -> Result<(), ConnectionManagerError> {
        let local_addr = self.get_local_ip()?;
        let mut connections = self.connections.write().await;

        if !connections.contains_key(peer_addr) {
            connections.insert(*peer_addr, connection);

            return Ok(());
        }

        if local_addr < *peer_addr {
            connection.close();
            return Ok(());
        }

        let old = connections.remove(peer_addr).unwrap();
        connections.insert(*peer_addr, connection);
        old.close();

        Ok(())
    }

    pub async fn start_listening(self: &Arc<Self>, cancel: CancellationToken) {
        let mut join_handle = JoinSet::new();

        match self.get_local_ip() {
            Ok(addr) => info!("BitTorrent service started on {}", addr),
            Err(_) => panic!("failed to retrive local address"),
        }

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    join_handle.shutdown().await;
                    break;
                },
                Some(incoming) = async {  self.endpoint.inner().accept().await} => {
                    let handshake_timeout = self.options.handshake_timeout;

                    if self.endpoint.inner().open_connections() >= self.options.max_connections {
                        info!("max connections reached: {}", self.options.max_connections);
                        incoming.refuse();
                        continue;
                    }

                    let weak_manager = Arc::downgrade(self);

                    join_handle.spawn(async move {
                        let manager = match weak_manager.upgrade() {
                            Some(s) => s,
                            None => return,
                        };

                        let conn_result = timeout(handshake_timeout, incoming).await;

                        let Ok(Ok(conn)) = conn_result else {
                            warn!(
                                "handshake failed, timout of {} seconds exceeded",
                                handshake_timeout.as_secs()
                            );
                            return;
                        };

                        let next_id = manager.next_id();
                        let peer_addr = conn.remote_address();

                        let connection = match Connection::new(
                            next_id,
                            peer_addr.clone(),
                            conn,
                            manager.message_tx.clone(),
                            manager.on_disconnect_handler(next_id)
                        ).await {
                            Ok(c) => c,
                            Err(e) => {
                                error!("failed to listen to new connections: {}", e.to_string());
                                return;
                            },
                        };

                        if let Err(e) = manager.append_connection(connection, &peer_addr).await {
                            debug!("failed to append connection: {}", e.to_string());
                        }
                    });
                }
            }
        }
    }

    fn is_self_connection(&self, peer_addr: &SocketAddr, local_addr: &SocketAddr) -> bool {
        if peer_addr.port() != local_addr.port() {
            return false;
        }

        let peer_ip = peer_addr.ip();
        let local_ip = local_addr.ip();

        peer_ip.is_loopback() || peer_ip.is_unspecified() || peer_ip == local_ip
    }

    pub async fn connect(&self, peer: &Peer) -> Result<(), ConnectionManagerError> {
        let peer_addr = peer.address;
        let local_addr = self.get_local_ip()?;

        if self.is_self_connection(&peer.address, &local_addr) {
            return Err(ConnectionManagerError::SelfConnectionError())?;
        }

        {
            let connections = self.connections.read().await;
            let dialing = self.connecting.read().await;

            if connections.contains_key(&peer_addr) || dialing.contains(&peer_addr) {
                return Ok(());
            }
        }

        let mut dialing = self.connecting.write().await;

        if dialing.contains(&peer_addr) {
            return Ok(());
        }

        dialing.insert(peer_addr);
        drop(dialing);

        let handshake = async {
            let id = self.next_id();
            let connecting = self
                .endpoint
                .inner()
                .connect(peer.address, "localhost")?
                .await?;

            let connection = Connection::new(
                id,
                peer_addr.clone(),
                connecting,
                self.message_tx.clone(),
                self.on_disconnect_handler(id),
            )
            .await?;

            self.append_connection(connection, &peer_addr).await?;
            Ok::<_, ConnectionManagerError>(())
        }
        .await;

        let mut dialing = self.connecting.write().await;
        dialing.remove(&peer_addr);
        drop(dialing);

        handshake
    }

    pub async fn send(
        &self,
        peer: &Peer,
        msg: impl Into<Message>,
    ) -> Result<(), ConnectionManagerError> {
        let local_addr = self.get_local_ip()?;

        if self.is_self_connection(&peer.address, &local_addr) {
            return Err(ConnectionManagerError::SelfConnectionError());
        }

        let connections = {
            let conns = self.connections.read().await;
            conns.get(&peer.address).cloned()
        };

        let Some(connection) = connections else {
            error!("failed to send control message");

            return Err(ConnectionManagerError::EstablishError(
                peer.address.to_string(),
            ));
        };

        connection.send_control(msg.into()).await?;

        Ok(())
    }

    pub async fn request(
        &self,
        peer: &Peer,
        msg: impl Into<Message>,
    ) -> Result<Vec<u8>, ConnectionManagerError> {
        let local_addr = self.get_local_ip()?;

        if self.is_self_connection(&peer.address, &local_addr) {
            return Err(ConnectionManagerError::SelfConnectionError());
        }

        let connection = {
            let conns = self.connections.read().await;
            conns.get(&peer.address).cloned()
        };

        let Some(connection) = connection else {
            error!("failed to send request and receive response");

            return Err(ConnectionManagerError::EstablishError(
                peer.address.to_string(),
            ));
        };

        let response = connection.request(msg.into()).await?;

        Ok(response)
    }

    pub async fn shutdown(&self) {
        let connections: Vec<_> = {
            let mut conns = self.connections.write().await;
            conns.drain().map(|(_, c)| c).collect()
        };

        for conn in connections {
            conn.close();
        }
    }
}
