use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use futures::{StreamExt, stream::FuturesUnordered};
use rand::RngExt;
use thiserror::Error;
use tokio::{sync::RwLock, time::sleep};

use crate::{
    bit_torrent::certs::{CertError, KeyIdentity, PubKeyCert, Signer, Verifier},
    dht::{
        BootStrap, Key, Node, OrdDistance, RequestId, RpcError,
        auth::{AuthError, AuthSecret, DIFFICULTY, PoW, SecretSalt},
        message::{
            DhtMessageError, DhtRequest, DhtResponse, KademliaData, PeerResponse, RpcRequest,
        },
        node,
        route_table::{InsertResult, KBUCKET_MAX, RouteTable},
        rpc::RpcHandler,
    },
    network::ConnectionManagerError,
    types::Hash32Bytes,
};

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

    #[error(transparent)]
    RpcError(#[from] RpcError),
}

pub struct Kademlia {
    certificate: KeyIdentity,
    secret_salt: Arc<RwLock<SecretSalt>>,

    rpc_handler: Arc<RpcHandler>,
    pub route_table: Arc<RwLock<RouteTable>>,
    pub store: Arc<RwLock<HashMap<Key, KademliaData>>>,
}

impl Kademlia {
    pub fn new(
        addr: &str,
        rpc_handler: Arc<RpcHandler>,
        certificate: KeyIdentity,
    ) -> Result<Self, KademliaError> {
        let host = Node::new(Key::new(certificate.pub_key()), addr)?;
        let route_table = RouteTable::new(host.clone());

        let dht = Self {
            certificate,
            secret_salt: Arc::new(RwLock::new(SecretSalt::new())),
            rpc_handler: rpc_handler,
            route_table: Arc::new(RwLock::new(route_table)),
            store: Arc::new(RwLock::new(HashMap::new())),
        };

        Ok(dht)
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

    // FIXME maybe do this in a seperate thread if not will block while doing the POW
    pub async fn join_network(&self, bootstrap: &BootStrap) -> Result<(), KademliaError> {
        let signed_secret = self.request_challange(bootstrap).await?;
        let bootstrap = self.submit_challange(bootstrap, &signed_secret).await?;

        self.acknowledge(&bootstrap).await;

        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        self.node_lookup(&host_id).await?;
        self.refresh_buckets(Duration::from_millis(50)).await;

        Ok(())
    }

    async fn request_challange(&self, bootstrap: &BootStrap) -> Result<PoW, KademliaError> {
        let bootstrap = bootstrap.node();
        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        let result = self
            .rpc_handler
            .request(
                bootstrap,
                DhtRequest::RequestChallange {
                    sender_id: host_id.clone(),
                },
                Duration::from_secs(5),
            )
            .await?;

        let DhtResponse::Challange {
            challange,
            difficulty,
        } = result
        else {
            return Err(RpcError::UnexpectedResponse)?;
        };

        let mut secret = PoW::generate(host_id.pub_key(), challange, difficulty);
        secret.sign(&self.certificate)?;

        Ok(secret)
    }

    async fn submit_challange(
        &self,
        bootstrap: &BootStrap,
        secret: &PoW,
    ) -> Result<Node, KademliaError> {
        let bootstrap = bootstrap.node();
        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        let Some(signature) = secret.signature.clone() else {
            return Err(AuthError::UnAuthorized())?;
        };

        let result = self
            .rpc_handler
            .request(
                bootstrap,
                DhtRequest::SubmitChallange {
                    sender_id: host_id,
                    challange: secret.value,
                    nonce: secret.nonce,
                    signature: signature,
                },
                Duration::from_secs(5),
            )
            .await?;

        let DhtResponse::Welcome {
            bootstrap_id,
            nonce,
            signature,
        } = result
        else {
            return Err(RpcError::UnexpectedResponse)?;
        };

        if !KeyIdentity::verify(bootstrap_id.pub_key(), &nonce.to_be_bytes(), &signature) {
            return Err(AuthError::RoguePeer())?;
        }

        Ok(Node::from_socket(bootstrap_id, bootstrap.addr))
    }

    async fn refresh_buckets(&self, interval: Duration) {
        const BUCKET_RANGE: usize = 40; // global coverage

        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        for index in 0..BUCKET_RANGE {
            let refresh_key = host_id.randomize(index);
            let _ = self.node_lookup(&refresh_key).await;
            sleep(interval).await;
        }
    }

    // TODO implement tokens, on store and find_node and find_value

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
                DhtRequest::Ping { sender_id: host_id },
                Duration::from_secs(5),
            )
            .await?;

        info!("pong");

        let DhtResponse::Pong {
            receiver_id: target_id,
        } = result
        else {
            return Err(RpcError::UnexpectedResponse)?;
        };

        if target.id != target_id {
            info!(
                "target: {} != expected: {}",
                target.id.hex(),
                target_id.hex()
            );
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
                        self.acknowledge(&target).await;

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
                    sender_id: host_id,
                    lookup_id: lookup_key,
                },
                Duration::from_secs(5),
            )
            .await?;

        let DhtResponse::Peers {
            receiver_id: sender_id,
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

    // TODO implement announce_torrent -> store for Info Hash (inside bqti)
    pub async fn store(&self, key: Key, value: KademliaData) -> Result<(), KademliaError> {
        let closest_nodes = self.node_lookup(&key).await?;

        if closest_nodes.is_empty() {
            return Err(KademliaError::NoNodesFound());
        }

        let mut future_stores = FuturesUnordered::new();
        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        for target in closest_nodes {
            let host_id = host_id.clone();
            let key = key.clone();
            let value = value.clone();

            future_stores.push(async move {
                let res = self
                    .rpc_handler
                    .request(
                        &target,
                        DhtRequest::Store {
                            sender_id: host_id,
                            key,
                            data: value,
                        },
                        Duration::from_secs(5),
                    )
                    .await;

                (target, res)
            });
        }

        while let Some((target, result)) = future_stores.next().await {
            match result {
                Ok(DhtResponse::Pong {
                    receiver_id: sender_id,
                }) if target.id == sender_id => {
                    info!("announce succedded");
                }
                _ => {
                    {
                        let mut route_table = self.route_table.write().await;
                        route_table.remove(&target);
                    }

                    warn!("failed to store on node: ?");
                }
            }
        }

        {
            let mut dht_store = self.store.write().await;
            dht_store.insert(key, value)
        };

        Ok(())
    }

    // TODO implement get_torrent -> find value for Info Hash (inside bqti)
    pub async fn find_value(&self, key: &Key) -> Result<Option<KademliaData>, KademliaError> {
        const ALPHA: usize = 3;
        const K: usize = KBUCKET_MAX;

        let mut futures_rpcs = FuturesUnordered::new();
        let mut visited_nodes = HashSet::<Key>::new();
        let mut candidates = {
            let route_table = self.route_table.read().await;
            route_table.get_closest_nodes(key, K)
        };

        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        loop {
            candidates.sort_by_key(|node| key.distance(&node.id));
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
                let host_id = host_id.clone();

                futures_rpcs.push(async move {
                    let result = self
                        .rpc_handler
                        .request(
                            &target,
                            DhtRequest::FindValue {
                                sender_id: host_id,
                                key: key.clone(),
                            },
                            Duration::from_secs(5),
                        )
                        .await;

                    (target, result)
                });
            }

            if futures_rpcs.is_empty() {
                break;
            }

            if let Some((target, result)) = futures_rpcs.next().await {
                match result {
                    Ok(DhtResponse::Value { receiver_id, value }) if target.id == receiver_id => {
                        return Ok(Some(value));
                    }
                    Ok(DhtResponse::Peers { receiver_id, peers }) if target.id == receiver_id => {
                        for peer in peers {
                            let node = Node::from(&peer);

                            if node.id != host_id && !visited_nodes.contains(&node.id) {
                                candidates.push(node);
                            }
                        }
                    }
                    _ => {
                        {
                            let mut route_table = self.route_table.write().await;
                            route_table.remove(&target);
                        }

                        warn!("node failed to find_value, removing from route table");
                        candidates.retain(|node| node.id != target.id);
                    }
                }
            }

            candidates.sort_by_key(|n| key.distance(&n.id));

            let finished = candidates
                .iter()
                .take(K)
                .all(|n| visited_nodes.contains(&n.id));

            if finished && futures_rpcs.is_empty() {
                break;
            }
        }

        Ok(None)
    }

    // TODO implement PEX

    // backend
    pub async fn handle_request(
        &self,
        request: RpcRequest,
        src: SocketAddr,
    ) -> Result<(), KademliaError> {
        match request.payload {
            DhtRequest::Ping { sender_id } => {
                self.handle_ping(request.id, &Node::from_socket(sender_id, src))
                    .await
            }
            DhtRequest::FindNode {
                sender_id,
                lookup_id,
            } => {
                self.handle_find_node(request.id, &Node::from_socket(sender_id, src), lookup_id)
                    .await
            }
            DhtRequest::Store {
                sender_id,
                key,
                data,
            } => {
                self.handle_store(request.id, &Node::from_socket(sender_id, src), key, data)
                    .await
            }
            DhtRequest::FindValue { sender_id, key } => {
                self.handle_find_value(request.id, &Node::from_socket(sender_id, src), key)
                    .await
            }
            DhtRequest::RequestChallange { sender_id } => {
                self.handle_request_challange(request.id, &Node::from_socket(sender_id, src))
                    .await
            }
            DhtRequest::SubmitChallange {
                sender_id,
                challange,
                nonce,
                signature,
            } => {
                self.handle_submit_challange(
                    request.id,
                    &Node::from_socket(sender_id, src),
                    challange,
                    nonce,
                    signature,
                )
                .await
            }
        }
    }

    async fn handle_ping(&self, request_id: RequestId, sender: &Node) -> Result<(), KademliaError> {
        self.acknowledge(sender).await;

        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        info!("ping");

        self.rpc_handler
            .reply(
                &sender,
                request_id,
                DhtResponse::Pong {
                    receiver_id: host_id,
                },
            )
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

        self.acknowledge(sender).await;

        self.rpc_handler
            .reply(
                &sender,
                request_id,
                DhtResponse::Peers {
                    receiver_id: host_id,
                    peers: closest_nodes
                        .iter()
                        .map(PeerResponse::from)
                        .collect::<Vec<_>>(),
                },
            )
            .await?;

        Ok(())
    }

    async fn handle_store(
        &self,
        request_id: RequestId,
        sender: &Node,
        key: Key,
        data: KademliaData,
    ) -> Result<(), KademliaError> {
        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        self.acknowledge(sender).await;

        {
            let mut dht_store = self.store.write().await;
            dht_store.insert(key, data);
        }

        self.rpc_handler
            .reply(
                &sender,
                request_id,
                DhtResponse::Pong {
                    receiver_id: host_id,
                },
            )
            .await?;

        Ok(())
    }

    async fn handle_find_value(
        &self,
        request_id: RequestId,
        sender: &Node,
        key: Key,
    ) -> Result<(), KademliaError> {
        let (host_id, record) = {
            let route_table = self.route_table.read().await;
            let dht_store = self.store.read().await;

            (route_table.host.id.clone(), dht_store.get(&key).cloned())
        };

        let response = match record {
            Some(value) => DhtResponse::Value {
                receiver_id: host_id,
                value,
            },
            None => {
                let closest_nodes = {
                    let route_table = self.route_table.read().await;
                    route_table.get_closest_nodes(&key, KBUCKET_MAX)
                };

                DhtResponse::Peers {
                    receiver_id: host_id,
                    peers: closest_nodes
                        .iter()
                        .map(PeerResponse::from)
                        .collect::<Vec<_>>(),
                }
            }
        };

        self.rpc_handler
            .reply(&sender, request_id, response)
            .await?;

        Ok(())
    }

    async fn handle_request_challange(
        &self,
        request_id: RequestId,
        sender: &Node,
    ) -> Result<(), KademliaError> {
        let difficulty = DIFFICULTY;
        let secret_salt = {
            let secret = self.secret_salt.read().await;
            *secret
        };

        let challange: u32 =
            SecretSalt::calculate_challenge(&sender.id.pub_key(), &sender.addr.ip(), &secret_salt);

        self.rpc_handler
            .reply(
                &sender,
                request_id,
                DhtResponse::Challange {
                    challange,
                    difficulty,
                },
            )
            .await?;

        Ok(())
    }

    async fn handle_submit_challange(
        &self,
        request_id: RequestId,
        sender: &Node,
        pow: Hash32Bytes,
        nonce: u32,
        pow_sign: Vec<u8>,
    ) -> Result<(), KademliaError> {
        let secret_salt = {
            let secret = self.secret_salt.read().await;
            *secret
        };

        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        let challange =
            SecretSalt::calculate_challenge(sender.id.pub_key(), &sender.addr.ip(), &secret_salt);

        let secret = PoW::new(pow, challange, nonce, DIFFICULTY, pow_sign);

        if !secret.verify(sender.id.pub_key()) {
            return Err(AuthError::RoguePeer())?;
        }

        let bootstrap_nonce: u32 = rand::rng().random();
        let bootstrap_sign = self.certificate.sign(&bootstrap_nonce.to_be_bytes())?;

        self.rpc_handler
            .reply(
                &sender,
                request_id,
                DhtResponse::Welcome {
                    bootstrap_id: host_id,
                    nonce: bootstrap_nonce,
                    signature: bootstrap_sign,
                },
            )
            .await?;

        Ok(())
    }
}
