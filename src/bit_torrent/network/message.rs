use std::net::SocketAddr;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MessageError {
    #[error("failed to parse message")]
    ParseFailed(),
}

pub struct Packet(pub Message, pub SocketAddr);

impl Packet {
    pub fn new(message: Message, socket_addr: SocketAddr) -> Self {
        Self(message, socket_addr)
    }
}

#[derive(Debug)]
pub enum Message {
    KeepAlive,
    DHT(Vec<u8>),
    PEX(Vec<u8>),
    Standard(Vec<u8>),
}

impl Message {
    pub fn to_bytes(&self) -> Vec<u8> {
        let (id, payload) = match self {
            Message::KeepAlive => return vec![0, 0, 0, 0],
            Message::DHT(bytes) => (b'd', bytes),
            Message::PEX(bytes) => (0x14, bytes),
            Message::Standard(std_msg) => (19, std_msg),
        };

        let mut buf = Vec::new();
        let length = (1 + payload.len()) as u32;
        buf.extend_from_slice(&length.to_be_bytes());
        buf.push(id);
        buf.extend_from_slice(&payload);
        buf
    }

    pub fn from_bytes(id: u8, payload: &[u8]) -> Result<Self, MessageError> {
        let payload = payload.to_vec();

        let message = match id {
            0x14 => Message::PEX(payload),
            b'd' => Message::DHT(payload),
            19 => Message::Standard(payload),
            _ => return Err(MessageError::ParseFailed()),
        };

        Ok(message)
    }
}
