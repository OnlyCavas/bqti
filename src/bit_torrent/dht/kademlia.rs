use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use thiserror::Error;
use tokio::sync::RwLock;

use crate::{
    dht::{
        Key, Node, RequestId, RpcError,
        message::{DhtMessageError, DhtRequest, DhtResponse, RpcRequest},
        node,
        route_table::{self, RouteTable},
        rpc::RpcHandler,
    },
    network::ConnectionManagerError,
};

#[derive(Debug, Error)]
pub enum KademliaError {
    #[error(transparent)]
    NodeError(#[from] node::NodeError),

    #[error(transparent)]
    ConnectionError(#[from] ConnectionManagerError),

    #[error(transparent)]
    DhtMessageError(#[from] DhtMessageError),

    #[error(transparent)]
    RpcError(#[from] RpcError),
}

pub enum KademliaData {
    Peer(SocketAddr),
    Value(Vec<u8>),
}

pub struct Kademlia {
    rpc_handler: Arc<RpcHandler>,
    pub route_table: Arc<RwLock<RouteTable>>,
    pub store: Arc<RwLock<HashMap<Key, KademliaData>>>,
}

impl Kademlia {
    pub fn new(addr: &str, rpc_handler: Arc<RpcHandler>) -> Result<Self, KademliaError> {
        let host = Node::new(addr)?;
        let route_table = RouteTable::new(host.clone());

        let dht = Self {
            rpc_handler: rpc_handler,
            route_table: Arc::new(RwLock::new(route_table)),
            store: Arc::new(RwLock::new(HashMap::new())),
        };

        Ok(dht)
    }

    // client
    pub async fn ping(&self, target: &Node) -> Result<(), KademliaError> {
        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        let result = self
            .rpc_handler
            .request(
                target,
                DhtRequest::Ping { node_id: host_id },
                Duration::from_secs(5),
            )
            .await?;

        info!("pong");

        let DhtResponse::Pong { node_id: target_id } = result else {
            return Err(RpcError::UnexpectedResponse)?;
        };

        if target.id != target_id {
            return Err(RpcError::UnexpectedResponse)?;
        }

        Ok(())
    }

    // backend
    pub async fn handle_request(
        &self,
        request: RpcRequest,
        src: SocketAddr,
    ) -> Result<(), KademliaError> {
        match request.payload {
            DhtRequest::Ping { node_id } => self.handle_ping(request.id, node_id, src).await,
        }
    }

    async fn handle_ping(
        &self,
        request_id: RequestId,
        key: Key,
        source: SocketAddr,
    ) -> Result<(), KademliaError> {
        let sender_node = Node::from_socket(key, source);

        {
            let mut route_table = self.route_table.write().await;
            route_table.insert_node(&sender_node).await;
        };

        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        info!("ping");

        self.rpc_handler
            .reply(
                &sender_node,
                request_id,
                DhtResponse::Pong { node_id: host_id },
            )
            .await?;

        Ok(())
    }
}
