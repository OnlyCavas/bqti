use std::{collections::HashSet, net::SocketAddr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    bit_torrent::{
        bencode::{self, BencodeError},
        certs::Signature,
    },
    dht::{Key, Node, RequestId, auth::Token},
    network::Message,
};

#[derive(Debug, Error)]
pub enum DhtMessageError {
    #[error("failed to parse dht message")]
    ParseFailed(),

    #[error(transparent)]
    BenEncodeError(#[from] BencodeError),

    #[error("invalid payload")]
    InvalidPayload,

    #[error("failed to persist data")]
    StoreFail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEnvelope<T> {
    pub id: RequestId,
    pub payload: T,
}

pub type RpcRequest = RpcEnvelope<DhtRequest>;
pub type AuthRpcRequest = RpcEnvelope<AuthDhtRequest>;

pub type RpcResponse = RpcEnvelope<DhtResponse>;

impl<T: Serialize> RpcEnvelope<T> {
    pub fn new(id: RequestId, payload: T) -> Self {
        Self { id, payload }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, DhtMessageError> {
        Ok(bencode::encode(self)?)
    }
}

impl<T: for<'de> Deserialize<'de>> RpcEnvelope<T> {
    pub fn from_bytes(data: &[u8]) -> Result<Self, DhtMessageError> {
        Ok(bencode::decode(data)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KademliaData {
    Peers(HashSet<SocketAddr>),
    Value(Vec<u8>),
}

impl KademliaData {
    pub fn merge(&mut self, other: KademliaData) {
        match (self, other) {
            (KademliaData::Peers(existing), KademliaData::Peers(new)) => {
                existing.extend(new);
            }
            (existing, new) => *existing = new,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthDhtRequest {
    Ping,
    FindNode { lookup_id: Key },
    FindValue { key: Key },
    Store { key: Key, data: KademliaData },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DhtRequest {
    RequestChallange {
        sender_id: Key,
    },
    SubmitChallange {
        sender_id: Key,
        challange: [u8; 32],
        nonce: u32,
        signature: Signature,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DhtResponse {
    Pong {
        receiver_id: Key,
    },
    Value {
        receiver_id: Key,
        value: KademliaData,
    },
    Peers {
        receiver_id: Key,
        peers: Vec<PeerResponse>,
    },
    Challange {
        challange: u32,
        difficulty: u32,
    },
    Welcome {
        token: Token,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DhtPacket {
    Request {
        token: Token,
        envelop: AuthRpcRequest,
    },
    HandShake(RpcRequest),
    Response(RpcResponse),
}

impl DhtPacket {
    pub fn to_bytes(&self) -> Result<Vec<u8>, DhtMessageError> {
        Ok(bencode::encode(self)?)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, DhtMessageError> {
        Ok(bencode::decode(data)?)
    }
}

impl TryFrom<DhtPacket> for Message {
    type Error = DhtMessageError;

    fn try_from(value: DhtPacket) -> Result<Self, Self::Error> {
        Ok(Message::DHT(value.to_bytes()?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerResponse {
    pub id: Key,
    pub addr: SocketAddr,
}

impl From<&PeerResponse> for Node {
    fn from(value: &PeerResponse) -> Self {
        Node::from_socket(value.id.clone(), value.addr)
    }
}
