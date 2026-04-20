use std::{net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use quinn::ConnectionError as QuicConnectionError;
use thiserror::Error;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::network::message::{Message, Packet};

const BUFFER_SIZE: u32 = 1024 * 1024;
pub type OnDisconnect = Arc<dyn Fn(SocketAddr) + Send + Sync + 'static>;

#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("message too big: {0} bytes")]
    MessageLimit(u32),

    #[error("network error (IO): {0}")]
    Io(#[from] std::io::Error),

    #[error("internal channel closed")]
    DispatcherClosed,

    #[error(transparent)]
    ConnectionError(#[from] QuicConnectionError),

    #[error("protocol error: {0}")]
    Protocol(String),
}

#[derive(Default)]
pub struct QuicServerOpts {
    pub connection_limit: Option<usize>,
}

#[async_trait]
pub trait ControlStream {
    async fn send_control(&self, message: Message) -> Result<(), ConnectionError>;
}

#[async_trait]
pub trait BidirectionalStream {
    async fn request(&self, request: Message) -> Result<Vec<u8>, ConnectionError>;
}

pub struct Connection {
    pub id: u64,
    connection: quinn::Connection,
    control_stream: Arc<Mutex<Option<quinn::SendStream>>>,
    cancellation_token: CancellationToken,
}

impl Connection {
    fn extract_protocol(connection: &quinn::Connection) -> String {
        connection
            .handshake_data()
            .and_then(|d| d.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
            .and_then(|d| d.protocol)
            .map_or_else(
                || "<none>".into(),
                |x| String::from_utf8_lossy(&x).into_owned(),
            )
    }

    pub async fn new(
        id: u64,
        peer: SocketAddr,
        quic: quinn::Connection,
        dispatcher: mpsc::Sender<Packet>,
        on_disconnect: OnDisconnect,
    ) -> Result<Arc<Self>, ConnectionError> {
        let cancellation_token = CancellationToken::new();

        let remote = quic.remote_address();
        let protocol = Self::extract_protocol(&quic);
        let control_stream = quic.open_uni().await?;

        let connection = Arc::new(Self {
            id,
            connection: quic,
            cancellation_token,
            control_stream: Arc::new(Mutex::new(Some(control_stream))),
        });

        let task_connection = connection.clone();
        let span = info_span!(
            "connection",
            remote = %remote,
            protocol = %protocol
        );

        tokio::spawn(
            async move {
                if let Err(e) = task_connection
                    .handle_connection(peer, dispatcher, on_disconnect)
                    .await
                {
                    error!("connection failed: {}", e);
                }
            }
            .instrument(span),
        );

        Ok(connection)
    }

    async fn handle_connection(
        self: Arc<Self>,
        peer: SocketAddr,
        dispatcher: mpsc::Sender<Packet>,
        on_disconnect: OnDisconnect,
    ) -> Result<(), ConnectionError> {
        info!("connection established");

        loop {
            tokio::select! {
                _ = self.cancellation_token.cancelled() => {
                    break;
                }

                stream = self.connection.accept_uni() => {
                    let recv = match stream {
                        Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                            info!("connection closed");
                            break;
                        }
                        Err(_) => {
                            break;
                        },
                        Ok(s) => s,
                    };

                    let stream_dispatch = dispatcher.clone();
                    let remote_addr = self.connection.remote_address();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_uni_stream(recv, stream_dispatch, remote_addr).await {
                            warn!("failed to handle stream: {}", e);
                        }
                    });
                },

                stream = self.connection.accept_bi() => {
                    let (send, recv) = match stream {
                        Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                            info!("connection closed");
                            break;
                        }
                        Err(_) => {
                            break;
                        },
                        Ok(s) => s,
                    };

                    let stream_dispatch = dispatcher.clone();
                    let remote_addr = self.connection.remote_address();
                    let cancellation_token = self.cancellation_token.child_token();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_bidi_stream(
                            send,
                            recv,
                            stream_dispatch,
                            remote_addr,
                            cancellation_token,
                        ).await
                        {
                            warn!("failed to handle bidi stream: {}", e);
                        }
                    });

                },
            }
        }

        on_disconnect(peer);

        Ok(())
    }

    async fn parse_incoming(recv: &mut quinn::RecvStream) -> Result<Message, ConnectionError> {
        let mut header = [0u8; 4];

        recv.read_exact(&mut header)
            .await
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        let length = u32::from_be_bytes(header);

        match length {
            0 => return Ok(Message::KeepAlive),
            len if len > BUFFER_SIZE => return Err(ConnectionError::MessageLimit(len)),
            _ => (),
        };

        let mut message_id = [0u8; 1];
        recv.read_exact(&mut message_id)
            .await
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        let payload_len = (length - 1) as usize;
        let mut payload_bytes = vec![0u8; payload_len];

        recv.read_exact(&mut payload_bytes)
            .await
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        let message = Message::from_bytes(message_id[0], &payload_bytes)
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        Ok(message)
    }

    async fn handle_uni_stream(
        mut recv: quinn::RecvStream,
        dispatcher: mpsc::Sender<Packet>,
        remote_addr: SocketAddr,
    ) -> Result<(), ConnectionError> {
        loop {
            let message = Self::parse_incoming(&mut recv).await?;

            dispatcher
                .send(Packet::new(message, remote_addr))
                .await
                .map_err(|_| ConnectionError::DispatcherClosed)?;
        }
    }

    async fn handle_bidi_stream(
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
        dispatcher: mpsc::Sender<Packet>,
        remote_addr: SocketAddr,
        cancellation_token: CancellationToken,
    ) -> Result<(), ConnectionError> {
        let message = Self::parse_incoming(&mut recv).await?;
        let (send_rx, recv_rx) = oneshot::channel::<Vec<u8>>();

        dispatcher
            .send(Packet::with_reply(message, remote_addr, send_rx))
            .await
            .map_err(|_| ConnectionError::DispatcherClosed)?;

        let reply = tokio::select! {
            _ = cancellation_token.cancelled() => return Ok(()),
            result = recv_rx => match result {
                Ok(reply) => reply,
                Err(_) => return Ok(()),
            }
        };

        send.write_all(&reply)
            .await
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        send.finish()
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        Ok(())
    }

    pub fn close(&self) {
        self.cancellation_token.cancel();
        self.connection.close(0u32.into(), b"shutdown");
    }
}

#[async_trait]
impl ControlStream for Connection {
    async fn send_control(&self, message: Message) -> Result<(), ConnectionError> {
        let mut control = self.control_stream.lock().await;

        if control.is_none() {
            let stream = self
                .connection
                .open_uni()
                .await
                .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

            *control = Some(stream);
        }

        let send = control.as_mut().unwrap();

        if let Err(e) = send.write_all(&message.to_bytes()).await {
            *control = None;

            return Err(ConnectionError::Protocol(e.to_string()));
        }

        Ok(())
    }
}

#[async_trait]
impl BidirectionalStream for Connection {
    async fn request(&self, request: Message) -> Result<Vec<u8>, ConnectionError> {
        let (mut send, mut recv) = self
            .connection
            .open_bi()
            .await
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        send.write_all(&request.to_bytes())
            .await
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        send.finish()
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        let response = recv
            .read_to_end(BUFFER_SIZE as usize)
            .await
            .map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        Ok(response)
    }
}
