use std::net::SocketAddr;

// TODO generate peer id, hash 20 or 32?
pub struct Peer {
    pub id: String,
    pub address: SocketAddr, // identitier inside quic
}

impl Peer {
    pub fn new(server_name: &str, addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            id: server_name.to_string(),
            address: addr.parse()?,
        })
    }
}
