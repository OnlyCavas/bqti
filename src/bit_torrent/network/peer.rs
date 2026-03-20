use std::net::SocketAddr;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PeerError {
    #[error(transparent)]
    FailParse(#[from] std::net::AddrParseError),
}

// TODO generate peer id, hash 20 or 32?
pub struct Peer {
    pub id: String,
    pub address: SocketAddr,
}

impl Peer {
    pub fn new(server_name: &str, addr: &str) -> Result<Self, PeerError> {
        Ok(Self {
            id: server_name.to_string(),
            address: addr.parse()?,
        })
    }
}
