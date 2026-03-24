use std::net::SocketAddr;

use thiserror::Error;

use crate::{
    dht::{Key, message::PeerResponse},
    network::Peer,
};

#[derive(Debug, Error)]
pub enum NodeError {
    #[error(transparent)]
    FailParse(#[from] std::net::AddrParseError),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: Key,
    pub addr: SocketAddr,
}

impl Node {
    pub fn new(addr: &str) -> Result<Self, NodeError> {
        let node = Self {
            id: Key::new(&[0u8; 32]),
            addr: addr.parse()?,
        };

        Ok(node)
    }

    pub fn from_socket(key: Key, socket: SocketAddr) -> Self {
        Self {
            id: key,
            addr: socket,
        }
    }
}

impl From<&Node> for PeerResponse {
    fn from(value: &Node) -> Self {
        Self {
            id: value.id.clone(),
            addr: value.addr,
        }
    }
}

impl From<&Node> for Peer {
    fn from(value: &Node) -> Self {
        Peer {
            id: value.id.hex(),
            address: value.addr,
        }
    }
}
