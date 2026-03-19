use quinn::{Connection, RecvStream, SendStream};
use tokio::{
    sync::mpsc::{self, Sender},
    task::JoinHandle,
};

use crate::bit_torrent::network::{
    connection::{Connector, QuicManager, StreamPair},
    peer::Peer,
};

#[derive(Debug)]
pub enum Message {
    KeepAlive,
    DHT(Vec<u8>),
    PEX(Vec<u8>),
    Standard(Vec<u8>),
}

impl Message {
    fn parse(id: u8, payload: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let payload = payload.to_vec();

        let message = match id {
            0x14 => Message::PEX(payload),
            b'd' => Message::DHT(payload),
            19 => Message::Standard(payload),
            _ => return Err("failed to parse message".into()),
        };

        Ok(message)
    }
}

pub struct PeerSession {
    pub peer: Peer,
    connection: Option<Connection>,
    loop_handle: Option<JoinHandle<()>>,
}

impl PeerSession {
    pub fn new(peer: Peer) -> Self {
        Self {
            peer,
            connection: None,
            loop_handle: None,
        }
    }

    async fn handle_incoming_request(
        _send: SendStream,
        mut recv: RecvStream,
        message_dispatcher: Sender<Message>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            let mut len_bytes = [0u8; 4];
            recv.read_exact(&mut len_bytes).await?;
            let length = u32::from_be_bytes(len_bytes);

            if length == 0 {
                // FIX it's a keep alive event
                continue;
            }

            if length > 1024 * 1024 {
                return Err("Message too large".into());
            }

            // second read message id
            let mut id_bytes = [0u8; 1];
            recv.read_exact(&mut id_bytes).await?;
            let message_id = id_bytes[0];

            // second read payload
            let payload_len = (length - 1) as usize;
            let mut payload_bytes = vec![0u8; payload_len];
            recv.read_exact(&mut payload_bytes).await?;

            let message = Message::parse(message_id, &payload_bytes)?;
            if let Err(e) = message_dispatcher.send(message).await {
                warn!("failed to dispatch the incoming request, {}", e.to_string());
            }
        }
    }

    pub async fn listening(
        &mut self,
        dest: Peer,
    ) -> Result<mpsc::Receiver<Message>, Box<dyn std::error::Error>> {
        const MESSAGE_LIMIT: usize = 100;

        if self.connection.is_some() {
            return Err("already listening".into());
        }

        let manager = QuicManager::new(self.peer.address)?;
        let (connection, mut rx) = manager.connect(dest).await?;

        let (tx, message_rx) = mpsc::channel::<Message>(MESSAGE_LIMIT);

        let handle = tokio::spawn(async move {
            while let Some((send, recv)) = rx.recv().await {
                let dispatcher = tx.clone();

                if let Err(e) = PeerSession::handle_incoming_request(send, recv, dispatcher).await {
                    warn!("Error, {}", e.to_string());
                }
            }
        });

        self.connection = Some(connection);
        self.loop_handle = Some(handle);

        Ok(message_rx)
    }
}
