use std::sync::Arc;

use quinn::{RecvStream, SendStream};
use thiserror::Error;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::Instrument;

use crate::network::message::Message;

pub type OnDisconnect = Arc<dyn Fn(String) + Send + Sync + 'static>;

#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("message too big: {0} bytes")]
    MessageLimit(u32),

    #[error("Erro de rede (IO): {0}")]
    Io(#[from] std::io::Error),

    #[error("O canal interno de mensagens fechou")]
    DispatcherClosed,

    #[error("Erro no protocolo BitTorrent: {0}")]
    Protocol(String),
}

#[derive(Default)]
pub struct QuicServerOpts {
    pub connection_limit: Option<usize>,
}

pub struct Connection {
    connection: quinn::Connection,
    handle: JoinHandle<()>,
}

impl Connection {
    fn connection_span(connection: &quinn::Connection) -> tracing::Span {
        let protocol = connection
            .handshake_data()
            .and_then(|d| d.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
            .and_then(|d| d.protocol)
            .map_or_else(
                || "<none>".into(),
                |x| String::from_utf8_lossy(&x).into_owned(),
            );

        info_span!(
            "connection",
            remote = %connection.remote_address(),
            protocol = %protocol
        )
    }

    pub async fn spawn(
        peer: String,
        connection: quinn::Connection,
        dispatcher: mpsc::Sender<Message>,
        on_disconnect: OnDisconnect,
    ) -> Result<Self, ConnectionError> {
        let peer_id = peer.clone();
        let connection_tx = connection.clone();

        let handle = tokio::spawn(
            async move {
                if let Err(e) = Self::handle_connection(connection_tx, dispatcher).await {
                    error!("connection failed: {}", e);
                }

                on_disconnect(peer_id);
            }
            .instrument(Self::connection_span(&connection)),
        );

        Ok(Self { connection, handle })
    }

    async fn handle_connection(
        connection: quinn::Connection,
        stream_dispatch: mpsc::Sender<Message>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        async {
            info!("connection established");

            loop {
                let stream = connection.accept_bi().await;

                let stream = match stream {
                    Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                        info!("connection closed");
                        return Ok(());
                    }
                    Err(e) => return Err(e),
                    Ok(stream) => stream,
                };

                let stream_dispatch = stream_dispatch.clone();
                tokio::spawn(async move {
                    if let Err(e) = Self::handle_request(stream.0, stream.1, stream_dispatch).await
                    {
                        warn!("failed to handle stream: {}", e);
                    }
                });
            }
        }
        .instrument(Self::connection_span(&connection))
        .await?;

        Ok(())
    }

    async fn handle_request(
        _send: SendStream,
        mut recv: RecvStream,
        message_dispatcher: mpsc::Sender<Message>,
    ) -> Result<(), ConnectionError> {
        loop {
            let mut len_bytes = [0u8; 4];

            recv.read_exact(&mut len_bytes)
                .await
                .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

            let length = u32::from_be_bytes(len_bytes);

            if length == 0 {
                message_dispatcher
                    .send(Message::KeepAlive)
                    .await
                    .map_err(|_| ConnectionError::DispatcherClosed)?;

                return Ok(());
            }

            if length > 1024 * 1024 {
                return Err(ConnectionError::MessageLimit(length));
            }

            let mut id_bytes = [0u8; 1];
            recv.read_exact(&mut id_bytes)
                .await
                .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

            let message_id = id_bytes[0];

            let payload_len = (length - 1) as usize;
            let mut payload_bytes = vec![0u8; payload_len];

            recv.read_exact(&mut payload_bytes)
                .await
                .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

            let message = Message::from_bytes(message_id, &payload_bytes)
                .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

            if let Err(e) = message_dispatcher.send(message).await {
                warn!("failed to dispatch... {}", e);
                return Err(ConnectionError::DispatcherClosed);
            }
        }
    }

    pub async fn close(self) -> Result<(), ConnectionError> {
        self.connection.close(0u32.into(), b"procedural shutdown");

        self.handle
            .await
            .map_err(|_| ConnectionError::DispatcherClosed)?;

        Ok(())
    }

    pub async fn send_message(&self, msg: Message) -> Result<(), ConnectionError> {
        let (mut send, _) = self
            .connection
            .open_bi()
            .await
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        let bytes = msg.to_bytes();

        send.write_all(&bytes)
            .await
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        send.finish()
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        Ok(())
    }
}
