use std::net::SocketAddr;

use rand::Rng;
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
    pub fn new(key: Key, addr: &str) -> Result<Self, NodeError> {
        let node = Self {
            id: key,
            addr: addr.parse()?,
        };

        Ok(node)
    }

    pub fn random(addr: &str) -> Result<Self, NodeError> {
        let mut random_bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut random_bytes);
        let id = Key::new(&random_bytes);

        let node = Self {
            id: id,
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

pub struct BootStrap(Node);

impl BootStrap {
    pub fn new(addr: &str) -> Result<Self, NodeError> {
        Ok(Self(Node::from_socket(Key::new(&[0u8; 32]), addr.parse()?)))
    }

    pub fn from_socket(socket: SocketAddr) -> Self {
        Self(Node::from_socket(Key::new(&[0u8; 32]), socket))
    }

    pub fn node(&self) -> &Node {
        &self.0
    }
}
