use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    bit_torrent::bencode::{self, BencodeError},
    network::Message,
};

pub type BepId = Vec<u8>;

#[derive(Debug, Error)]
pub enum StandardMessageError {
    #[error(transparent)]
    BencodeError(#[from] BencodeError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StandardMessage {
    Handshake { info_hash: Vec<u8>, peer_id: BepId },
    Bitfield(Vec<u8>),
    Interested,
    NotInterested,
    Choke,
    Unchoke,
    Request { index: u32 },
    Piece { index: u32, data: Vec<u8> },
    Have { index: u32 },
}

impl StandardMessage {
    pub fn to_bytes(&self) -> Result<Vec<u8>, StandardMessageError> {
        Ok(bencode::encode(self)?)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StandardMessageError> {
        Ok(bencode::decode(bytes)?)
    }
}

impl TryFrom<StandardMessage> for Message {
    type Error = StandardMessageError;

    fn try_from(value: StandardMessage) -> Result<Self, Self::Error> {
        Ok(Message::Standard(value.to_bytes()?))
    }
}
