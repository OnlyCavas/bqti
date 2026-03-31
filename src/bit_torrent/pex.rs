use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::{
    bit_torrent::bencode::{self, BencodeError},
    dht::Key,
    network::{ConnectionManager, ConnectionManagerError, Message, Peer},
};

const MAX_SWARM_SIZE: usize = 200;
const MAX_PEX_DELTA: usize = 50;
const GOSSIP_INTERVAL: Duration = Duration::from_secs(60);

pub type InfoHash = Key;

#[derive(Debug, Error)]
pub enum PexError {
    #[error(transparent)]
    BencodeError(#[from] BencodeError),

    #[error(transparent)]
    ConnectionManagerError(#[from] ConnectionManagerError),

    #[error("failed to handle request")]
    HandleFailed(),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PexMessage {
    pub info_hash: InfoHash,
    pub added: Vec<SocketAddr>,
    pub dropped: Vec<SocketAddr>,
}

impl PexMessage {
    pub fn to_bytes(&self) -> Result<Vec<u8>, PexError> {
        Ok(bencode::encode(self)?)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PexError> {
        Ok(bencode::decode(bytes)?)
    }
}

impl TryFrom<PexMessage> for Message {
    type Error = PexError;

    fn try_from(value: PexMessage) -> Result<Self, Self::Error> {
        Ok(Message::PEX(value.to_bytes()?))
    }
}

struct PexSession {
    last_sent: HashSet<SocketAddr>,
}

pub struct PexHandler {
    swarms: RwLock<HashMap<InfoHash, HashSet<SocketAddr>>>, // active peers within a torrent swarm
    sessions: RwLock<HashMap<SocketAddr, PexSession>>,      // calculate deltas
    connection_manager: Arc<ConnectionManager>,
}

// NOTE if .torrent has private flag, pex protocol must be disabled
impl PexHandler {
    fn create(connection_manager: Arc<ConnectionManager>) -> Self {
        Self {
            swarms: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            connection_manager,
        }
    }

    pub fn new(connection_manager: Arc<ConnectionManager>) -> Arc<Self> {
        let pex_handler = Arc::new(PexHandler::create(connection_manager));
        let weak_prt = Arc::downgrade(&pex_handler);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(GOSSIP_INTERVAL);

            loop {
                interval.tick().await;

                let Some(handler) = weak_prt.upgrade() else {
                    return;
                };

                let pairs: Vec<(InfoHash, SocketAddr)> = {
                    let swarms = handler.swarms.read().await;
                    swarms
                        .iter()
                        .flat_map(|(info_hash, peers)| {
                            peers.iter().map(|peer| (info_hash.clone(), *peer))
                        })
                        .collect()
                };

                for (info_hash, peer) in pairs {
                    let Some(delta) = handler.delta(&info_hash, &peer).await else {
                        continue;
                    };

                    if let Err(_) = handler.send_message(peer, delta).await {
                        warn!("disconnecting, pex peer for not responding");
                    }
                }
            }
        });

        pex_handler
    }

    pub async fn handle_incoming<F, Fut>(
        &self,
        incoming: PexMessage,
        socket: &SocketAddr,
        on_peer_discovered: F,
    ) -> Result<(), PexError>
    where
        F: Fn(InfoHash, SocketAddr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.announce(incoming.info_hash.clone(), *socket).await;

        let Some(handshake) = self.message(&incoming.info_hash, socket).await else {
            return Err(PexError::HandleFailed());
        };

        self.send_message(*socket, handshake).await?;

        for addr in &incoming.added {
            let info_hash = incoming.info_hash.clone();

            self.announce(info_hash.clone(), *addr).await;
            tokio::spawn(on_peer_discovered(info_hash, *addr));
        }

        for addr in &incoming.dropped {
            self.disconnect(&incoming.info_hash, addr).await;
        }

        Ok(())
    }

    async fn send_message(&self, peer: SocketAddr, pex: PexMessage) -> Result<(), PexError> {
        let info_hash = pex.info_hash.clone();
        let message = Message::try_from(pex)?;

        match self
            .connection_manager
            .send(&Peer::from_socket(peer), message)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => {
                self.disconnect(&info_hash, &peer).await;

                Ok(())
            }
        }
    }

    pub async fn announce(&self, info_hash: InfoHash, addr: SocketAddr) {
        let mut swarms = self.swarms.write().await;
        let swarm = swarms.entry(info_hash).or_default();

        if swarm.len() >= MAX_SWARM_SIZE {
            return;
        }

        swarm.insert(addr);
    }

    pub async fn disconnect(&self, info_hash: &InfoHash, addr: &SocketAddr) {
        {
            let mut swarms = self.swarms.write().await;

            if let Some(peers) = swarms.get_mut(info_hash) {
                peers.remove(addr);
            }
        }

        self.sessions.write().await.remove(addr);
    }

    pub async fn get_peers(&self, info_hash: &InfoHash) -> Vec<SocketAddr> {
        let swarms = self.swarms.read().await;

        swarms
            .get(info_hash)
            .map(|s| s.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    }

    pub async fn message(&self, info_hash: &InfoHash, peer: &SocketAddr) -> Option<PexMessage> {
        let current = {
            let swarms = self.swarms.read().await;
            swarms
                .get(info_hash)?
                .iter()
                .cloned()
                .collect::<HashSet<_>>()
        };

        if current.is_empty() {
            return None;
        }

        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(
                *peer,
                PexSession {
                    last_sent: current.clone(),
                },
            );
        }

        let message = PexMessage {
            info_hash: info_hash.clone(),
            added: current.into_iter().take(MAX_PEX_DELTA).collect(),
            dropped: vec![],
        };

        Some(message)
    }

    pub async fn delta(&self, info_hash: &InfoHash, peer: &SocketAddr) -> Option<PexMessage> {
        let budget = MAX_PEX_DELTA;

        let current = {
            let swarms = self.swarms.read().await;
            swarms
                .get(info_hash)?
                .iter()
                .cloned()
                .collect::<HashSet<_>>()
        };

        let mut sessions = self.sessions.write().await;
        let session = sessions.entry(*peer).or_insert_with(|| PexSession {
            last_sent: HashSet::new(),
        });

        let added: Vec<_> = current
            .difference(&session.last_sent)
            .cloned()
            .take(budget)
            .collect();

        let dropped: Vec<_> = session
            .last_sent
            .difference(&current)
            .cloned()
            .take(budget.saturating_sub(added.len()))
            .collect();

        if added.is_empty() && dropped.is_empty() {
            return None;
        }

        session.last_sent = current;
        let message = PexMessage {
            info_hash: info_hash.clone(),
            added,
            dropped,
        };

        Some(message)
    }
}
