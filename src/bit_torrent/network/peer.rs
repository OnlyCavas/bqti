use std::net::SocketAddr;

pub struct Peer {
    pub id: String,
    pub address: SocketAddr,
}

impl Peer {
    pub fn new(server_name: &str, addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            id: server_name.to_string(),
            address: addr.parse()?,
        })
    }
}
