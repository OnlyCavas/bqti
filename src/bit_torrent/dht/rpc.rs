use std::{
    collections::HashMap,
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
        message::{DhtMessageError, DhtPacket, DhtRequest, DhtResponse, RpcRequest, RpcResponse},
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
    connection_manager: Arc<ConnectionManager>,
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

    pub async fn dispatch(&self, packet: DhtPacket) -> Option<RpcRequest> {
        match packet {
            DhtPacket::Request(request) => Some(request),
            DhtPacket::Response(response) => {
                let pending_result = {
                    let mut pending = self.pending.lock().await;
                    pending.remove(&response.id)
                };

                let Some(response_tx) = pending_result else {
                    warn!("stale response, no pending request for id={}", response.id);
                    return None;
                };

                if response_tx.send(response.payload).is_err() {
                    error!("failed to send rpc response: ");
                }

                None
            }
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

    // FIX if timeout exceded, breaks the connection flow
    pub async fn request(
        &self,
        peer: &Node,
        request: DhtRequest,
        tout: Duration,
    ) -> Result<DhtResponse, RpcError> {
        let peer = peer.into();
        let id = self.alloc_id();
        let envelope = RpcRequest::new(id, request);
        let packet = DhtPacket::Request(envelope);

        let (tx, rx) = oneshot::channel::<DhtResponse>();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        let connect_and_send = {
            self.connection_manager.connect(&peer).await?;
            self.connection_manager
                .send(&peer, Message::try_from(packet)?)
        }
        .await;

        if let Err(e) = connect_and_send {
            {
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
            }

            return Err(e.into());
        }

        match timeout(tout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(RpcError::Timeout),
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
