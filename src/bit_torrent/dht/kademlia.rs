use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

use futures::{StreamExt, stream::FuturesUnordered};
use thiserror::Error;
use tokio::{sync::RwLock, time::sleep};

use crate::{
    bit_torrent::certs::{CertError, KeyIdentity, PublicKey},
    dht::{
        BootStrap, DhtPacket, Key, Node, OrdDistance, RequestId, RpcError,
        auth::{AuthError, AuthManager, Authorizable, Challenge, DIFFICULTY, Evidence, PoW},
        message::{
            AuthDhtRequest, AuthRpcRequest, DhtMessageError, DhtRequest, DhtResponse, KademliaData,
            PeerResponse, RpcRequest,
        },
        node,
        route_table::{InsertResult, KBUCKET_MAX, RouteTable},
        rpc::RpcHandler,
        store::{DHTStore, PRUNE_CHECK_DURATION},
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
    auth: Arc<AuthManager>,
    rpc_handler: Arc<RpcHandler>,
    store: Arc<RwLock<DHTStore>>,
    pub route_table: Arc<RwLock<RouteTable>>,
}

impl Kademlia {
    pub fn new(
        addr: &str,
        rpc_handler: Arc<RpcHandler>,
        certificate: KeyIdentity,
    ) -> Result<Arc<Self>, KademliaError> {
        let host = Node::new(Key::new(certificate.pub_key()), addr)?;
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
            .handshake(
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
        secret.sign(self.auth.certificate())?;

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
            .handshake(
                bootstrap,
                DhtRequest::SubmitChallange {
                    sender_id: host_id.clone(),
                    challange: secret.value,
                    nonce: secret.nonce,
                    signature: signature,
                },
                Duration::from_secs(5),
            )
            .await?;

        let DhtResponse::Welcome { token } = result else {
            return Err(RpcError::UnexpectedResponse)?;
        };

        if !token.verify_for(&host_id.pub_key()) {
            return Err(AuthError::RoguePeer())?;
        }

        let boostrap = Node::from_socket(Key::new(&token.issuer), bootstrap.addr);
        self.auth.store_token(token).await;

        Ok(boostrap)
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

    // client
    pub async fn ping(&self, target: &Node) -> Result<(), KademliaError> {
        let result = self
            .auth_request(target, AuthDhtRequest::Ping, Duration::from_secs(5))
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
        let result = self
            .auth_request(
                target,
                AuthDhtRequest::FindNode {
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

        for target in closest_nodes {
            let key = key.clone();
            let value = value.clone();

            future_stores.push(async move {
                let res = self
                    .auth_request(
                        &target,
                        AuthDhtRequest::Store { key, data: value },
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

                futures_rpcs.push(async move {
                    let result = self
                        .auth_request(
                            &target,
                            AuthDhtRequest::FindValue { key: key.clone() },
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

    pub async fn handle_packet(
        &self,
        packet: DhtPacket,
        src: SocketAddr,
    ) -> Result<(), KademliaError> {
        match packet {
            DhtPacket::Request { token, envelop } => {
                if !token.verify() {
                    return Err(AuthError::InvalidToken())?;
                }

                self.auth.check_rate(&token.sender()).await?;
                self.handle_request(token.sender(), envelop, src).await
            }
            DhtPacket::HandShake(rpc) => self.handle_handshake(rpc, src).await,
            _ => panic!("shouldn't receive any response packages"),
        }
    }

    async fn handle_handshake(
        &self,
        request: RpcRequest,
        src: SocketAddr,
    ) -> Result<(), KademliaError> {
        match request.payload {
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

    async fn handle_request(
        &self,
        sender_id: Key,
        request: AuthRpcRequest,
        src: SocketAddr,
    ) -> Result<(), KademliaError> {
        let sender = Node::from_socket(sender_id, src);

        match request.payload {
            AuthDhtRequest::Ping => self.handle_ping(request.id, &sender).await,
            AuthDhtRequest::FindNode { lookup_id } => {
                self.handle_find_node(request.id, &sender, lookup_id).await
            }
            AuthDhtRequest::FindValue { key } => {
                self.handle_find_value(request.id, &sender, key).await
            }
            AuthDhtRequest::Store { key, data } => {
                self.handle_store(request.id, &sender, key, data).await
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
        let challange = self
            .auth
            .challange(&sender.id.pub_key(), &sender.addr.ip())
            .await;

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
        let challange = self
            .auth
            .challange(sender.id.pub_key(), &sender.addr.ip())
            .await;

        let secret = PoW::new(pow, challange, nonce, DIFFICULTY, pow_sign);
        let token = self.auth.issue_token(sender, &secret).await?;

        self.rpc_handler
            .reply(&sender, request_id, DhtResponse::Welcome { token })
            .await?;

        Ok(())
    }
}
