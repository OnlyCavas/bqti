use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use futures::{StreamExt, stream::FuturesUnordered};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::{
    dht::{
        Key, Node, OrdDistance, RequestId, RpcError,
        message::{DhtMessageError, DhtRequest, DhtResponse, PeerResponse, RpcRequest},
        node,
        route_table::{KBUCKET_MAX, RouteTable},
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
    Peers(HashSet<SocketAddr>),
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

    pub async fn node_lookup(&self, lookup_key: &Key) -> Result<Vec<Node>, KademliaError> {
        const ALPHA: usize = 3;
        const K: usize = KBUCKET_MAX;

        let mut futures_rpcs = FuturesUnordered::new();
        let mut visited_nodes = HashSet::<Key>::new();
        let mut candidates = {
            let route_table = self.route_table.read().await;
            route_table.get_closest_nodes(lookup_key, K)
        };

        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        loop {
            candidates.sort_by_key(|node| lookup_key.distance(&node.id));
            candidates.dedup_by(|a, b| a.id == b.id);

            let take = ALPHA.saturating_sub(futures_rpcs.len());
            let request_batch = candidates
                .iter()
                .filter(|node| !visited_nodes.contains(&node.id))
                .take(take)
                .cloned()
                .collect::<Vec<_>>();

            for target in request_batch {
                visited_nodes.insert(target.id.clone());

                futures_rpcs.push(async move {
                    let result = self.find_node(&target, lookup_key.clone()).await;
                    (target, result)
                });
            }

            if futures_rpcs.is_empty() {
                break;
            }

            if let Some((target, result)) = futures_rpcs.next().await {
                match result {
                    Err(_) => {
                        {
                            let mut route_table = self.route_table.write().await;
                            route_table.remove(&target);
                        }

                        warn!(
                            "node failed to respond, removing it from table: {:?}",
                            target.id
                        );

                        candidates.retain(|node| node.id != target.id);
                    }
                    Ok(nodes) => {
                        {
                            let mut route_table = self.route_table.write().await;
                            route_table.insert_node(&target).await;
                        }

                        for node in nodes {
                            if node.id != host_id
                                && !visited_nodes.contains(&node.id)
                                && !candidates.iter().any(|c| c.id == node.id)
                            {
                                candidates.push(node);
                            }
                        }
                    }
                }
            }

            candidates.sort_by_key(|n| lookup_key.distance(&n.id));

            let finished = candidates
                .iter()
                .take(K)
                .all(|n| visited_nodes.contains(&n.id));

            if finished && futures_rpcs.is_empty() {
                break;
            }
        }

        candidates.truncate(KBUCKET_MAX);

        Ok(candidates)
    }

    pub async fn find_node(
        &self,
        target: &Node,
        lookup_key: Key,
    ) -> Result<Vec<Node>, KademliaError> {
        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        let result = self
            .rpc_handler
            .request(
                target,
                DhtRequest::FindNode {
                    node_id: host_id,
                    lookup_id: lookup_key,
                },
                Duration::from_secs(5),
            )
            .await?;

        let DhtResponse::Peers {
            node_id: sender_id,
            peers,
        } = result
        else {
            return Err(RpcError::UnexpectedResponse)?;
        };

        if target.id != sender_id {
            return Err(RpcError::UnexpectedResponse)?;
        }

        let nodes = peers.iter().map(Node::from).collect::<Vec<_>>();
        Ok(nodes)
    }

    // TODO implement store
    // TODO implement publish_torrent -> store for Info Hash (inside bqti)

    // TODO implement find_nodes
    // TODO implement find_value

    // TODO implement get_peers -> find value for Info Hash (inside bqti)

    // TODO implement PEX

    // backend
    pub async fn handle_request(
        &self,
        request: RpcRequest,
        src: SocketAddr,
    ) -> Result<(), KademliaError> {
        match request.payload {
            DhtRequest::Ping { node_id } => {
                self.handle_ping(request.id, &Node::from_socket(node_id, src))
                    .await
            }
            DhtRequest::FindNode { node_id, lookup_id } => {
                self.handle_find_node(request.id, &Node::from_socket(node_id, src), lookup_id)
                    .await
            }
        }
    }

    async fn handle_ping(&self, request_id: RequestId, sender: &Node) -> Result<(), KademliaError> {
        {
            let mut route_table = self.route_table.write().await;
            route_table.insert_node(&sender).await;
        };

        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        info!("ping");

        self.rpc_handler
            .reply(&sender, request_id, DhtResponse::Pong { node_id: host_id })
            .await?;

        Ok(())
    }

    async fn handle_find_node(
        &self,
        request_id: RequestId,
        sender: &Node,
        lookup: Key,
    ) -> Result<(), KademliaError> {
        let (host_id, closest_nodes) = {
            let route_table = self.route_table.read().await;

            (
                route_table.host.id.clone(),
                route_table.get_closest_nodes(&lookup, KBUCKET_MAX),
            )
        };

        {
            let mut route_table = self.route_table.write().await;
            route_table.insert_node(sender).await;
        }

        self.rpc_handler
            .reply(
                &sender,
                request_id,
                DhtResponse::Peers {
                    node_id: host_id,
                    peers: closest_nodes
                        .iter()
                        .map(PeerResponse::from)
                        .collect::<Vec<_>>(),
                },
            )
            .await?;

        Ok(())
    }
}
