use std::{net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::RwLock;

use crate::{
    bit_torrent::certs::{CertError, PublicKey},
    certs::ActiveKeyIdentity,
    dht::{
        BootStrap, DhtPacket, Key, Node, ProveChallenge, RpcError,
        auth::{AuthError, AuthManager},
        message::{AuthDhtRequest, DhtMessageError, DhtRequest, DhtResponse},
        node,
        route_table::{InsertResult, RouteTable},
        rpc::RpcHandler,
        store::{DHTStore, PRUNE_CHECK_DURATION},
    },
    network::{Connection, ConnectionAuth, ConnectionManagerError},
};

mod client;
mod server;

#[async_trait]
pub trait KademliaServer {
    async fn handle_packet(
        &self,
        packet: DhtPacket,
        src: SocketAddr,
        connection: Arc<Connection>,
    ) -> Result<(), KademliaError>;
}

#[async_trait]
pub trait KademliaClient {
    async fn join_network(&self, bootstrap: &BootStrap) -> Result<(), KademliaError>;
    async fn ping(&self, target: &Node) -> Result<(), KademliaError>;
}

#[derive(Debug, Error)]
pub enum KademliaError {
    #[error(transparent)]
    NodeError(#[from] node::NodeError),

    #[error("handshake failed")]
    AuthError(#[from] AuthError),

    #[error("failed to connect over to bootstrap node")]
    BootstrapFailed(),

    #[error("failed to find closest nodes")]
    NoNodesFound(),

    #[error(transparent)]
    ConnectionError(#[from] ConnectionManagerError),

    #[error(transparent)]
    CertErr(#[from] CertError),

    #[error(transparent)]
    DhtMessageError(#[from] DhtMessageError),

    #[error("no data with that key was found")]
    NoValue(),

    #[error(transparent)]
    RpcError(#[from] RpcError),
}

pub struct Kademlia {
    auth: Arc<AuthManager>,
    pub(crate) rpc_handler: Arc<RpcHandler>, // HACK shouldnt be public
    store: Arc<RwLock<DHTStore>>,
    pub route_table: Arc<RwLock<RouteTable>>,
}

impl Kademlia {
    pub fn new(
        rpc_handler: Arc<RpcHandler>,
        certificate: Arc<ActiveKeyIdentity>,
    ) -> Result<Arc<Self>, KademliaError> {
        let auth_manager = AuthManager::new(certificate, "kademlia")?;

        let local_addr = rpc_handler.get_local_addr()?;
        let host = Node::from_socket(Key::new(auth_manager.pub_key()), local_addr);
        let route_table = RouteTable::new(host.clone());

        let dht = Arc::new(Self {
            auth: auth_manager,
            rpc_handler: rpc_handler,
            route_table: Arc::new(RwLock::new(route_table)),
            store: Arc::new(RwLock::new(DHTStore::new())),
        });

        let weak_ptr = Arc::downgrade(&dht.store);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(PRUNE_CHECK_DURATION);

            loop {
                interval.tick().await;

                let Some(dht_lock) = weak_ptr.upgrade() else {
                    info!("prunning stopped");
                    return;
                };

                let mut store = dht_lock.write().await;
                store.prune();

                drop(store);
            }
        });

        Ok(dht)
    }

    async fn authenticate(&self, target: &Node, timeout: Duration) -> Result<(), KademliaError> {
        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        let token = self
            .auth
            .best_token()
            .await
            .ok_or(AuthError::UnAuthorized())?;

        let DhtResponse::HandshakeChallange { nonce } = self
            .rpc_handler
            .handshake(
                target,
                DhtRequest::RequestHandshake { sender_id: host_id },
                timeout,
            )
            .await?
        else {
            return Err(KademliaError::AuthError(AuthError::UnAuthorized()));
        };

        let mut token_bytes = token.hash().to_vec();
        token_bytes.extend_from_slice(&nonce.to_be_bytes());

        let Ok(signature) = self.auth.prover().sign(&token_bytes) else {
            return Err(KademliaError::AuthError(AuthError::InvalidToken()));
        };

        let DhtResponse::Pong { receiver_id } = self
            .rpc_handler
            .handshake(
                target,
                DhtRequest::SubmitHandshake {
                    token: token.clone(),
                    signature,
                },
                timeout,
            )
            .await?
        else {
            return Err(KademliaError::AuthError(AuthError::UnAuthorized()));
        };

        if receiver_id != target.id {
            return Err(KademliaError::AuthError(AuthError::RoguePeer()));
        }

        if let Some(conn) = self
            .rpc_handler
            .connection_manager
            .get_connection(&target.addr)
            .await
        {
            conn.authenticate(ConnectionAuth::Authenticated(token))
                .await;
        }

        Ok(())
    }

    async fn auth_request(
        &self,
        target: &Node,
        request: AuthDhtRequest,
        timeout: Duration,
    ) -> Result<DhtResponse, KademliaError> {
        let connection = self
            .rpc_handler
            .connection_manager
            .get_connection(&target.addr)
            .await;

        let needs_auth = match connection.as_ref() {
            Some(c) => !c.is_authenticated().await,
            None => true,
        };

        if needs_auth {
            self.authenticate(target, timeout).await?;
        }

        let request = self.rpc_handler.request(target, request, timeout).await?;
        Ok(request)
    }

    async fn acknowledge(&self, node: &Node) {
        loop {
            let insert_result = {
                let mut route_table = self.route_table.write().await;
                route_table.try_insert(node).await
            };

            match insert_result {
                InsertResult::None => break,
                InsertResult::Inserted => break,
                InsertResult::Split => continue,
                InsertResult::Ping(old, new) => match self.ping(&old).await {
                    Ok(_) => break,
                    Err(_) => {
                        let mut route_table = self.route_table.write().await;
                        route_table.replace(&new);
                    }
                },
            }
        }
    }

    // client
}
