use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    bit_torrent::bencode::{self, BencodeError},
    dht::{Key, RequestId},
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEnvelope<T> {
    pub id: RequestId,
    pub payload: T,
}

pub type RpcRequest = RpcEnvelope<DhtRequest>;
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
pub enum DhtRequest {
    Ping { node_id: Key },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DhtResponse {
    Pong { node_id: Key },
    Sopa,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DhtPacket {
    Request(RpcRequest),
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
