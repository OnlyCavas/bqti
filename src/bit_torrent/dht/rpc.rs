use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use thiserror::Error;
use tokio::{
    sync::{Mutex, oneshot},
    time::timeout,
};

use crate::{
    dht::{
        Node, RequestId,
        message::{
            AuthDhtRequest, DhtMessageError, DhtPacket, DhtRequest, DhtResponse, RpcEnvelope,
            RpcResponse,
        },
    },
    network::{ConnectionManager, ConnectionManagerError, Message},
};

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("timed out")]
    Timeout,

    #[error("channel closed")]
    ChannelClosed,

    #[error("unexpected response")]
    UnexpectedResponse,

    #[error(transparent)]
    Transport(#[from] ConnectionManagerError),

    #[error(transparent)]
    Message(#[from] DhtMessageError),
}

pub struct RpcHandler {
    pub connection_manager: Arc<ConnectionManager>, // HACK shouldn't be public
    pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<DhtResponse>>>>,
    next_id: AtomicU64,
}

impl RpcHandler {
    pub fn new(connection_manager: Arc<ConnectionManager>) -> Self {
        Self {
            connection_manager: connection_manager,
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(0),
        }
    }

    fn alloc_id(&self) -> RequestId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn get_local_addr(&self) -> Result<SocketAddr, ConnectionManagerError> {
        self.connection_manager.get_local_ip()
    }

    pub async fn dispatch(&self, packet: DhtPacket) -> Option<DhtPacket> {
        match packet {
            DhtPacket::Response(response) => {
                let pending_result = {
                    let mut pending = self.pending.lock().await;
                    pending.remove(&response.id)
                };

                let Some(response_tx) = pending_result else {
                    warn!("stale response, no pending request for id={}", response.id);
                    return None;
                };

                if let Err(_) = response_tx.send(response.payload) {
                    debug!("in-flight value response after early exit")
                }

                None
            }
            packet => Some(packet),
        }
    }

    pub async fn reply(
        &self,
        peer: &Node,
        id: RequestId,
        response: DhtResponse,
    ) -> Result<(), RpcError> {
        let peer = peer.into();
        let envelope = RpcResponse::new(id, response);
        let packet = DhtPacket::Response(envelope);

        self.connection_manager.connect(&peer).await?;

        self.connection_manager
            .send(&peer, Message::try_from(packet)?)
            .await?;

        Ok(())
    }

    pub async fn handshake(
        &self,
        peer: &Node,
        payload: DhtRequest,
        tout: Duration,
    ) -> Result<DhtResponse, RpcError> {
        self.handle_request(peer, tout, |id| {
            DhtPacket::HandShake(RpcEnvelope::new(id, payload))
        })
        .await
    }

    pub async fn request(
        &self,
        peer: &Node,
        payload: AuthDhtRequest,
        tout: Duration,
    ) -> Result<DhtResponse, RpcError> {
        self.handle_request(peer, tout, |id| DhtPacket::Request {
            envelop: RpcEnvelope::new(id, payload),
        })
        .await
    }

    pub async fn handle_request(
        &self,
        peer: &Node,
        tout: Duration,
        make_packet: impl FnOnce(RequestId) -> DhtPacket,
    ) -> Result<DhtResponse, RpcError> {
        let peer = peer.into();

        let id = self.alloc_id();
        let packet = make_packet(id);

        let (tx, rx) = oneshot::channel::<DhtResponse>();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        let connect_and_send = {
            self.connection_manager.connect(&peer).await?;

            self.connection_manager
                .send(&peer, Message::try_from(packet)?)
                .await
        };

        if let Err(e) = connect_and_send {
            {
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
            }

            return Err(e.into());
        }

        match timeout(tout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(RpcError::ChannelClosed),
            Err(_) => {
                {
                    let mut pending = self.pending.lock().await;
                    pending.remove(&id);
                }

                Err(RpcError::Timeout)
            }
        }
    }
}
