use std::net::SocketAddr;

use crate::{
    dht::{
        DhtPacket, DhtResponse, Kademlia, KademliaData, KademliaError, Key, Node, RequestId,
        RpcRequest,
        auth::{AuthError, Authorizable, DIFFICULTY, PoW},
        message::{AuthDhtRequest, AuthRpcRequest, DhtRequest, PeerResponse},
        route_table::KBUCKET_MAX,
    },
    types::Hash32Bytes,
};

impl Kademlia {
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
