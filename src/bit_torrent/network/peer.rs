use std::net::SocketAddr;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PeerError {
    #[error(transparent)]
    FailParse(#[from] std::net::AddrParseError),
}

#[derive(Debug)]
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
