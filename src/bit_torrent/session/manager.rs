use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use bqti_ipc::{Event, TorrentState};
use thiserror::Error;
use tokio::sync::{
    RwLock, broadcast,
    mpsc::{self, Sender, channel},
    oneshot,
};
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::torrent::metainfo::{InfoHash, Metainfo, TorrentFile},
    session::{
        StandardMessage, TorrentSessionError,
        bep::BepRouter,
        cache::{CachingMode, SessionCache},
        session::{SessionMode, TorrentSession},
        transition::Transition,
    },
    torrent::metainfo::Magnet,
};

const SESSION_EVENT_SIZE: usize = 32;
const TORRENT_QUEUE_SIZE: usize = 16;
const BROADCAST_IPC_EVENT_SIZE: usize = 32;

#[derive(Debug, Error)]
pub enum SessionManagerError {
    #[error("session is corrupted")]
    SessionCorrupted(),

    #[error("torrent already exists, {0}")]
    AlreadyExists(String),

    #[error(transparent)]
    TorrentSessionError(#[from] TorrentSessionError),

    #[error("session identified by {0}, doesn't exist")]
    NotFound(String),
}

pub enum SessionEvent {
    PieceVerified { total_pieces: u32, current: u32 },
    SeedStarted,
    Idle,
    PieceDownloaded { total_pieces: u32, current: u32 },
    DownloadCompleted { resource_path: String },
}

struct SessionManagerInner {
    by_hash: HashMap<InfoHash, Arc<TorrentSession>>,
    by_peer: HashMap<SocketAddr, Arc<TorrentSession>>,
}

struct QueuedTorrent {
    mode: SessionMode,
    torrent: Arc<TorrentFile>,
}

pub struct SessionManager {
    inner: Arc<RwLock<SessionManagerInner>>,
    queue_tx: Sender<QueuedTorrent>,
    bep_router: Arc<BepRouter>,
    ipc_tx: broadcast::Sender<Event>,
    cancellation_token: CancellationToken,
}

impl SessionManager {
    pub fn new(bep_router: Arc<BepRouter>) -> Self {
        let (queue_tx, queue_rx) = mpsc::channel(TORRENT_QUEUE_SIZE);
        let (ipc_tx, _) = broadcast::channel(BROADCAST_IPC_EVENT_SIZE);
        let token = CancellationToken::new();

        let inner = Arc::new(RwLock::new(SessionManagerInner {
            by_hash: HashMap::new(),
            by_peer: HashMap::new(),
        }));

        tokio::spawn(queue_worker(
            queue_rx,
            inner.clone(),
            bep_router.clone(),
            ipc_tx.clone(),
            token.clone(),
        ));

        let manager = Self {
            inner,
            bep_router,
            queue_tx: queue_tx.clone(),
            ipc_tx,
            cancellation_token: token,
        };

        tokio::spawn(restore_cache(queue_tx.clone()));

        manager
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.ipc_tx.subscribe()
    }

    pub async fn dispatch(
        &self,
        message: StandardMessage,
        source: SocketAddr,
        reply: Option<oneshot::Sender<Vec<u8>>>,
    ) {
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

        match self
            .bep_router
            .handle_request(session.clone(), message, source, reply)
            .await
        {
            Ok(_) => (),
            Err(super::BepRouterError::ConnectionManagerError(_)) => {
                session.terminate_with(&source).await;
                debug!("peer went down, terminating...");
            }
            Err(e) => warn!("unexpected error: {} ", e.to_string()),
        }
    }

    pub async fn add(
        &self,
        mode: SessionMode,
        torrent: Arc<TorrentFile>,
    ) -> Result<(), SessionManagerError> {
        let info_hash = torrent.info_hash();

        {
            let inner = self.inner.read().await;

            if inner.by_hash.contains_key(&info_hash) {
                return Err(SessionManagerError::AlreadyExists(info_hash.to_string()));
            }
        }

        self.queue_tx
            .send(QueuedTorrent { mode, torrent })
            .await
            .map_err(|_| SessionManagerError::SessionCorrupted())?;

        Ok(())
    }

    pub async fn get(&self, info_hash: &InfoHash) -> Option<Arc<TorrentSession>> {
        let inner = self.inner.read().await;
        inner.by_hash.get(info_hash).cloned()
    }

    pub async fn pause(&self, info_hash: &InfoHash) -> Result<(), SessionManagerError> {
        let Some(session) = self.get(info_hash).await else {
            return Err(SessionManagerError::NotFound(info_hash.to_string()));
        };

        session.transition(Transition::Pause).await?;
        Ok(())
    }

    pub async fn resume(&self, info_hash: &InfoHash) -> Result<(), SessionManagerError> {
        let Some(session) = self.get(info_hash).await else {
            return Err(SessionManagerError::NotFound(info_hash.to_string()));
        };

        session
            .transition(Transition::Resume {
                metafile: session.metadata.clone(),
            })
            .await?;

        Ok(())
    }

    pub async fn cancel(&self, info_hash: &InfoHash) -> Result<(), SessionManagerError> {
        let Some(session) = self.get(info_hash).await else {
            return Err(SessionManagerError::NotFound(info_hash.to_string()));
        };

        session.transition(Transition::Cancel).await?;
        Ok(())
    }

    pub async fn remove(&self, info_hash: &InfoHash) {
        let session = {
            let mut inner = self.inner.write().await;

            inner.by_hash.remove(info_hash).inspect(|session| {
                inner.by_peer.retain(|_, s| !Arc::ptr_eq(s, session));
            })
        };

        if let Some(session) = session {
            let mut state = session.state.write().await;

            match state.purge().await {
                Ok(_) => info!("purge complete"),
                Err(e) => error!("purge failed: {}", e),
            }
        }
    }
}

async fn restore_cache(queue_tx: mpsc::Sender<QueuedTorrent>) {
    let mut cache = SessionCache::load_from_cache().await;

    while let Some(cache_mode) = cache.next() {
        let (mode, torrent) = match cache_mode {
            CachingMode::Download { metafile } => (SessionMode::Download, metafile),
            CachingMode::Seed {
                user_space,
                metafile,
            } => (
                SessionMode::Seed {
                    source_dir: user_space,
                },
                metafile,
            ),
        };

        if queue_tx
            .send(QueuedTorrent { mode, torrent })
            .await
            .is_err()
        {
            warn!("failed to restore cache, at session level");
        }
    }
}

async fn queue_worker(
    mut rx: mpsc::Receiver<QueuedTorrent>,
    inner: Arc<RwLock<SessionManagerInner>>,
    bep_router: Arc<BepRouter>,
    ipc_tx: broadcast::Sender<Event>,
    token: CancellationToken,
) {
    while let Some(QueuedTorrent { mode, torrent }) = rx.recv().await {
        let (event_tx, event_rx) = channel(SESSION_EVENT_SIZE);

        if token.is_cancelled() {
            return;
        }

        tokio::spawn(session_events_worker(
            event_rx,
            torrent.clone(),
            ipc_tx.clone(),
            token.clone(),
        ));

        let session =
            match TorrentSession::new(mode, torrent.clone(), bep_router.clone(), event_tx).await {
                Ok(session) => session,
                Err(e) => {
                    error!(
                        "failed to create session for {}, {}",
                        torrent.info_hash().to_string(),
                        e.to_string()
                    );

                    continue;
                }
            };

        inner
            .write()
            .await
            .by_hash
            .insert(torrent.info_hash().clone(), session);
    }
}

async fn session_events_worker(
    mut event_rx: mpsc::Receiver<SessionEvent>,
    torrent: Arc<TorrentFile>,
    ipc_tx: broadcast::Sender<Event>,
    token: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                return;
            },

            Some(event) = event_rx.recv() => {
                let event: Event = match event {
                    SessionEvent::PieceVerified {
                        total_pieces,
                        current,
                    } => Event::SessionStateChanged {
                        info_hash: torrent.info_hash().to_string(),
                        name: torrent.name().to_string(),
                        state: TorrentState::Verifying {
                            verified: current,
                            total: total_pieces,
                        },
                    },
                    SessionEvent::SeedStarted => Event::ExposeTorrent {
                        info_hash: torrent.info_hash().to_string(),
                        magnet: torrent.magnet().to_string(),
                    },
                    SessionEvent::PieceDownloaded {
                        total_pieces,
                        current,
                    } => Event::SessionStateChanged {
                        info_hash: torrent.info_hash().to_string(),
                        name: torrent.name().to_string(),
                        state: TorrentState::Downloading {
                            current,
                            total_pieces,
                            download_rate: 0,
                        },
                    },
                    SessionEvent::DownloadCompleted { resource_path } => Event::DownloadComplted {
                        info_hash: torrent.info_hash().to_string(),
                        resource_path,
                    },
                    SessionEvent::Idle => Event::SessionStateChanged {
                        info_hash: torrent.info_hash().to_string(),
                        name: torrent.name().to_string(),
                        state: TorrentState::Paused,
                    },
                };

                let _ = ipc_tx.send(event);

            }
            else => return,
        }
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}
