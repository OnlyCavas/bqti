use std::{net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::RwLock;

use crate::{
    bit_torrent::certs::{CertError, KeyIdentity, PublicKey},
    dht::{
        BootStrap, DhtPacket, Key, Node, RpcError,
        auth::{AuthError, AuthManager},
        message::{AuthDhtRequest, DhtMessageError, DhtResponse},
        node,
        route_table::{InsertResult, RouteTable},
        rpc::RpcHandler,
        store::{DHTStore, PRUNE_CHECK_DURATION},
    },
    network::ConnectionManagerError,
};

mod client;
mod server;

#[async_trait]
pub trait KademliaServer {
    async fn handle_packet(&self, packet: DhtPacket, src: SocketAddr) -> Result<(), KademliaError>;
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
    rpc_handler: Arc<RpcHandler>,
    store: Arc<RwLock<DHTStore>>,
    pub route_table: Arc<RwLock<RouteTable>>,
}

impl Kademlia {
    pub fn new(
        rpc_handler: Arc<RpcHandler>,
        certificate: KeyIdentity,
    ) -> Result<Arc<Self>, KademliaError> {
        let local_addr = rpc_handler.get_local_addr()?;
        let host = Node::from_socket(Key::new(certificate.pub_key()), local_addr);
        let route_table = RouteTable::new(host.clone());

        let dht = Arc::new(Self {
            auth: AuthManager::new(certificate),
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

    async fn auth_request(
        &self,
        target: &Node,
        request: AuthDhtRequest,
        timeout: Duration,
    ) -> Result<DhtResponse, KademliaError> {
        let token = self
            .auth
            .best_token()
            .await
            .ok_or(AuthError::UnAuthorized())?;

        Ok(self
            .rpc_handler
            .request(target, token, request, timeout)
            .await?)
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
