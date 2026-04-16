use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use tokio::sync::RwLock;

use crate::{
    bit_torrent::torrent::metainfo::{InfoHash, Metainfo, TorrentFile},
    session::{
        StandardMessage,
        bep::BepRouter,
        session::{SessionMode, TorrentSession},
    },
};

// TODO get an snapshot

struct SessionManagerInner {
    by_hash: HashMap<InfoHash, Arc<TorrentSession>>,
    by_peer: HashMap<SocketAddr, Arc<TorrentSession>>,
}

pub struct SessionManager {
    inner: RwLock<SessionManagerInner>,
    bep_router: Arc<BepRouter>,
}

impl SessionManager {
    pub fn new(bep_router: Arc<BepRouter>) -> Self {
        Self {
            inner: RwLock::new(SessionManagerInner {
                by_hash: HashMap::new(),
                by_peer: HashMap::new(),
            }),

            bep_router,
        }
    }

    pub async fn dispatch(&self, message: StandardMessage, source: SocketAddr) {
        let (session, message) = match message {
            StandardMessage::Handshake { ref info_hash, .. } => {
                let info_hash = match InfoHash::try_from(info_hash.clone()) {
                    Ok(h) => h,
                    Err(_) => {
                        warn!("invalid info_hash in handshake from {}", source);
                        return;
                    }
                };

                let session = {
                    let mut inner = self.inner.write().await;

                    let Some(session) = inner.by_hash.get(&info_hash).cloned() else {
                        warn!("handshake from {} for unknown torrent, dropping", source);
                        return;
                    };

                    inner.by_peer.insert(source, session.clone());

                    session
                };

                (session, message)
            }
            msg => {
                let inner = self.inner.read().await;

                let Some(session) = inner.by_peer.get(&source) else {
                    warn!("message from unknown peer {}, dropping", source);
                    return;
                };

                (session.clone(), msg)
            }
        };

        if let Err(e) = self
            .bep_router
            .handle_request(session, message, source)
            .await
        {
            warn!("failed, bep request: {:?}", e);
        }
    }

    pub async fn add(&self, mode: SessionMode, torrent: &TorrentFile) -> bool {
        let mut inner = self.inner.write().await;
        let info_hash = torrent.info_hash();

        if inner.by_hash.contains_key(&info_hash) {
            debug!(
                "torrent {} already exists, ignoring",
                hex::encode(info_hash.as_ref())
            );

            return false;
        }

        let Ok(session) = TorrentSession::new(mode, torrent.clone(), self.bep_router.clone()).await
        else {
            return false;
        };

        inner.by_hash.insert(info_hash.clone(), session);
        true
    }

    pub async fn get(&self, info_hash: &InfoHash) -> Option<Arc<TorrentSession>> {
        let inner = self.inner.read().await;
        inner.by_hash.get(info_hash).cloned()
    }

    pub async fn remove(&self, info_hash: &InfoHash) {
        let mut inner = self.inner.write().await;

        if let Some(session) = inner.by_hash.remove(info_hash) {
            inner.by_peer.retain(|_, s| !Arc::ptr_eq(s, &session));
        }
    }
}
