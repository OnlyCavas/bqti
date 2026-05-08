use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use futures::{StreamExt, stream::FuturesUnordered};
use tokio::time::sleep;

use crate::dht::{
    BootStrap, DhtResponse, Kademlia, KademliaData, KademliaError, Key, Node, OrdDistance,
    RpcError,
    auth::{AuthError, Authorizable, Challenge, Evidence, PoW},
    kademlia::KademliaClient,
    message::{AuthDhtRequest, DhtRequest},
    route_table::KBUCKET_MAX,
};

#[allow(dead_code)]
const REFRESH_BUCKET_INTERVAL: Duration = Duration::from_millis(50);

const TIMEOUT_EXCEPTION: Duration = Duration::from_secs(30);

#[async_trait]
impl KademliaClient for Kademlia {
    async fn join_network(&self, bootstrap: &BootStrap) -> Result<(), KademliaError> {
        let signed_secret = self.request_challange(bootstrap).await?;
        let bootstrap = self.submit_challange(bootstrap, &signed_secret).await?;
        info!("bootstrapped");

        self.acknowledge(&bootstrap).await;

        let host_id = {
            let route_table = self.route_table.read().await;
            route_table.host.id.clone()
        };

        self.node_lookup(&host_id).await?;

        Ok(())
    }

    async fn ping(&self, target: &Node) -> Result<(), KademliaError> {
        let result = self
            .auth_request(target, AuthDhtRequest::Ping, TIMEOUT_EXCEPTION)
            .await?;

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
}

impl Kademlia {
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
                TIMEOUT_EXCEPTION,
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
                TIMEOUT_EXCEPTION,
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

    #[allow(dead_code)]
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

    async fn node_lookup(&self, lookup_key: &Key) -> Result<Vec<Node>, KademliaError> {
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

    async fn find_node(&self, target: &Node, lookup_key: Key) -> Result<Vec<Node>, KademliaError> {
        let result = self
            .auth_request(
                target,
                AuthDhtRequest::FindNode {
                    lookup_id: lookup_key,
                },
                TIMEOUT_EXCEPTION,
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

    pub async fn store(&self, key: Key, value: KademliaData) -> Result<(), KademliaError> {
        let closest_nodes = self.node_lookup(&key).await?;

        let mut future_stores = FuturesUnordered::new();

        for target in closest_nodes {
            let key = key.clone();
            let value = value.clone();

            future_stores.push(async move {
                let res = self
                    .auth_request(
                        &target,
                        AuthDhtRequest::Store { key, data: value },
                        TIMEOUT_EXCEPTION,
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
                            TIMEOUT_EXCEPTION,
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
}
