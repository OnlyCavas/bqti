use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{
    sync::{Mutex, RwLock, mpsc, oneshot},
    time::{Instant, interval_at, timeout},
};

use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::{
        chunks::{Reader, Writer},
        torrent::metainfo::{Metainfo, PieceIntegrity, TorrentFile},
    },
    dht::{Key, Node},
    network::ConnectionManagerError,
    session::{
        BepId, BepRouterError, StandardMessage, TorrentSessionError,
        bep::{BepRouter, PeerState, Pipeline},
        bit_field::BitField,
        manager::SessionEvent,
        state::TorrentState,
        transition::Transition,
    },
};

const BEP_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

pub enum SessionMode {
    Download,
    Seed { source_dir: PathBuf },
}

pub struct TorrentSession {
    bep_router: Arc<BepRouter>,

    pub(crate) metadata: Arc<TorrentFile>,
    pub(crate) state: RwLock<TorrentState>,
    pub(crate) bitfield: Arc<RwLock<BitField>>,

    peers: RwLock<HashMap<SocketAddr, PeerState>>,

    pub(crate) event_tx: mpsc::Sender<SessionEvent>,
    pub(crate) pending_writes: AtomicUsize,
}

pub struct PieceRequest {
    pub index: u32,
    pub respond: oneshot::Sender<Vec<u8>>,
}

impl TorrentSession {
    pub async fn new(
        mode: SessionMode,
        metadata: Arc<TorrentFile>,
        bep_router: Arc<BepRouter>,
        event_tx: mpsc::Sender<SessionEvent>,
    ) -> Result<Arc<Self>, TorrentSessionError> {
        let piece_count = metadata.piece_hashes().len();

        let session = Arc::new(Self {
            metadata: metadata.clone(),
            state: RwLock::new(TorrentState::default()),
            bitfield: Arc::new(RwLock::new(BitField::empty(piece_count))),
            bep_router: bep_router.clone(),
            peers: RwLock::new(HashMap::new()),
            event_tx,
            pending_writes: AtomicUsize::new(0),
        });

        match mode {
            SessionMode::Seed { source_dir } => {
                debug!("files found, loading pieces...");

                session
                    .transition(Transition::Seed {
                        path: source_dir,
                        metafile: metadata.clone(),
                    })
                    .await?;
            }
            SessionMode::Download => {
                info!("no files found, starting download");

                session
                    .transition(Transition::Download {
                        metafile: metadata.clone(),
                    })
                    .await?;
            }
        };

        let mut disc_rx = bep_router.subscribe_disconnects();
        let weak_session = Arc::downgrade(&session);

        tokio::spawn(async move {
            while let Ok(addr) = disc_rx.recv().await {
                if let Some(session) = weak_session.upgrade() {
                    session.terminate_with(&addr).await;
                }
            }
        });

        bep_router.start_peer_discovery(session.clone());

        Ok(session)
    }

    pub(crate) async fn send_event(&self, event: SessionEvent) {
        match self.event_tx.send(event).await {
            Ok(_) => (),
            Err(e) => warn!("failed to send (Session Event): {}", e.to_string()),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn spawn_interest_prober(
        self: &Arc<Self>,
        token: CancellationToken,
    ) -> Result<(), TorrentSessionError> {
        let weak_ptr = Arc::downgrade(self);

        tokio::spawn(async move {
            let start = tokio::time::Instant::now() + Duration::from_secs(2);
            let mut interval = tokio::time::interval_at(start, Duration::from_secs(15));

            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        break
                    },
                    _ = interval.tick() => {
                        let session = match weak_ptr.upgrade() {
                            Some(s) => s,
                            None => return,
                        };

                        let guard = session.peers.read().await;
                        let bitfield = session.get_bitfield().await;

                        for (peer, state) in guard.iter() {
                            let PeerState::Active(pipeline) = state else { continue };

                            info!("evaluating for interest {:?}", peer);

                            if pipeline.evaluate_interest(&bitfield).await {
                                match session.bep_router.send(*peer, StandardMessage::Interested).await {
                                    Ok(_) => (),
                                    Err(_) => warn!("failed to show interest"),
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    pub(crate) fn spawn_downloader(
        self: &Arc<Self>,
        user_space: &Path,
        mut downloader: mpsc::Receiver<(u32, Vec<u8>)>,
        cancellation_token: CancellationToken,
    ) -> Result<(), TorrentSessionError> {
        let weak_ptr = Arc::downgrade(self);
        let user_space = PathBuf::from(user_space);

        tokio::spawn(async move {
            let session = match weak_ptr.upgrade() {
                Some(s) => s,
                None => return,
            };

            let mut state = session.state.write().await;

            let Some(resources) = state.take_downloading() else {
                return;
            };

            drop(state);

            session.broadcast(StandardMessage::Interested).await;

            let piece_count = session.metadata.piece_hashes().len() - 1;
            let resources = Arc::new(resources);

            let (done_tx, mut done_rx) = oneshot::channel::<()>();
            let done_tx = Arc::new(Mutex::new(Some(done_tx)));

            let mut resume_interval = interval_at(
                Instant::now() + Duration::from_secs(20),
                Duration::from_secs(1),
            );

            loop {
                tokio::select! {
                    biased;

                    _ = cancellation_token.cancelled() => {
                        break;
                    }

                    _ = &mut done_rx => {
                        break;
                    }

                    _ = resume_interval.tick() => {
                        resources.persist_resume(&session).await;
                    },

                    message = downloader.recv() => match message {
                        None => {
                            break;
                        }
                        Some((index, data)) => {
                            let session = Arc::downgrade(&session);
                            let resources = Arc::downgrade(&resources);
                            let done_tx = done_tx.clone();

                            tokio::spawn(async move {
                                let session = match session.upgrade() {
                                    Some(s) => s,
                                    None => return,
                                };

                                let resources = match resources.upgrade() {
                                    Some(s) => s,
                                    None => return,
                                };

                                if session.metadata.verify_hash(index, &data).is_err() {
                                    session.pending_writes.fetch_sub(1, Ordering::Relaxed);
                                    return;
                                }

                                match resources.handler.write_piece(index, data).await {
                                    Ok(_) => {
                                        session.pending_writes.fetch_sub(1, Ordering::Relaxed);

                                        session.send_event(SessionEvent::PieceDownloaded {
                                            total_pieces: piece_count as u32,
                                            current: index,
                                        }).await;

                                        let current_bitfield = session.bitfield.read().await;

                                        if current_bitfield.count() != piece_count + 1 {
                                            return;
                                        }

                                        if let Some(tx) = done_tx.lock().await.take() {
                                            let _ = tx.send(());
                                        }
                                    }
                                    Err(e) => {
                                        session.pending_writes.fetch_sub(1, Ordering::Relaxed);
                                        warn!("failed to persist piece {}: {}", index, e);
                                    }
                                }
                            });
                        }
                    }
                }
            }

            resources.persist_resume(&session).await;

            drop(done_tx);
            drop(resources);

            session.broadcast(StandardMessage::NotInterested).await;

            if cancellation_token.is_cancelled() {
                return;
            }

            cancellation_token.cancel();

            debug!("finish downloading, trying to switch to seeding...");

            session
                .send_event(SessionEvent::DownloadCompleted {
                    resource_path: user_space.to_string_lossy().to_string(),
                })
                .await;

            match session
                .transition(Transition::Seed {
                    path: user_space,
                    metafile: session.metadata.clone(),
                })
                .await
            {
                Ok(_) => (),
                Err(e) => {
                    error!("failed to transition to seeding: {}", e.to_string());
                }
            }
        });

        Ok(())
    }

    pub(crate) fn spawn_seeder(
        self: &Arc<Self>,
        mut uploader: mpsc::Receiver<PieceRequest>,
        cancellation_token: CancellationToken,
    ) -> Result<(), TorrentSessionError> {
        let weak_ptr = Arc::downgrade(self);

        tokio::spawn(async move {
            let session = match weak_ptr.upgrade() {
                Some(s) => s,
                None => return,
            };

            session.send_event(SessionEvent::SeedStarted).await;

            let mut state = session.state.write().await;

            let Some(resources) = state.take_seeding() else {
                return;
            };

            drop(state);

            let resources = Arc::new(resources);
            debug!("started seeding...");

            loop {
                tokio::select! {
                    biased;
                    _ = cancellation_token.cancelled() => {
                        break;
                    }
                    message = uploader.recv() => match message {
                        None => {
                            info!("uploader is closed");
                            break;
                        }
                        Some(PieceRequest { index, respond }) => {
                            let resources = resources.clone();

                            tokio::spawn(async move {
                                match resources.handler.read_piece(index).await {
                                    Ok(data) => {
                                        let _ = respond.send(data);
                                    }
                                    Err(e) => warn!("failed to read piece {}: {}", index, e),
                                }
                            });
                        }
                    }
                }
            }

            session.broadcast(StandardMessage::Choke).await;

            if cancellation_token.is_cancelled() {
                return;
            }

            debug!("stopped seeding, state: idle");
            session.transition(Transition::Pause).await.ok();
        });

        Ok(())
    }
    pub async fn send_downloaded_piece(&self, index: u32, data: Vec<u8>) {
        self.pending_writes.fetch_add(1, Ordering::Relaxed);

        let state = self.state.read().await;

        let Some(tx) = state.downloader_tx() else {
            self.pending_writes.fetch_sub(1, Ordering::Relaxed);
            return;
        };

        if tx.send((index, data)).await.is_err() {
            self.pending_writes.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub async fn send_piece_request(&self, request: PieceRequest) {
        let state = self.state.read().await;

        let Some(tx) = state.uploader_tx() else {
            return;
        };

        if tx.send(request).await.is_err() {
            warn!("failed to send to downloader, channel closed");
            return;
        }
    }

    pub fn add_discovered_peers(self: &Arc<TorrentSession>, mut peers: Vec<SocketAddr>) {
        if peers.is_empty() {
            return;
        }

        peers.sort_unstable();
        peers.dedup();

        let weak_session = Arc::downgrade(self);

        for addr in peers {
            let weak_clone = weak_session.clone();

            tokio::spawn(async move {
                let session = match weak_clone.upgrade() {
                    Some(s) => s,
                    None => return,
                };

                {
                    let mut peer_state = session.peers.write().await;

                    if peer_state.contains_key(&addr) {
                        debug!(
                            "skipping reconnect to {}, state: {:?}",
                            addr,
                            peer_state.get(&addr)
                        );

                        return;
                    }

                    peer_state.insert(addr, PeerState::Connecting);
                }

                let handshake_fut = session.bep_router.handshake(session.clone(), addr);

                let Ok(result) = timeout(BEP_HANDSHAKE_TIMEOUT, handshake_fut).await else {
                    debug!("bep handshake timeout: {}", addr);
                    session.peers.write().await.remove(&addr);

                    return;
                };

                match result {
                    Ok(_) => debug!("bep handshake, connected: {}", addr),
                    Err(BepRouterError::ConnectionManagerError(
                        ConnectionManagerError::SelfConnectionError(),
                    )) => {
                        session.peers.write().await.remove(&addr);
                        debug!("ignoring self connection");
                    }
                    Err(e) => {
                        session.peers.write().await.remove(&addr);
                        debug!("bep handshake, failed: {} - {}", addr, e);
                    }
                }
            });
        }
    }
}

impl TorrentSession {
    pub async fn get_pipeline(&self, src: SocketAddr) -> Option<Arc<Pipeline>> {
        let peers = self.peers.read().await;

        match peers.get(&src) {
            Some(PeerState::Active(p)) => Some(p.clone()),
            _ => None,
        }
    }

    pub async fn set_bitfield(&self, index: u32) -> BitField {
        let mut bitfield = self.bitfield.write().await;
        bitfield.set(index as usize);
        bitfield.clone()
    }

    pub async fn get_bitfield(&self) -> BitField {
        let bitfield = self.bitfield.read().await;
        bitfield.clone()
    }

    pub async fn add_piece(&self, index: u32) -> bool {
        let mut bitfield = self.bitfield.write().await;

        if bitfield.have(index as usize) {
            return false;
        }

        bitfield.set(index as usize);
        true
    }

    pub async fn get_pending_peer(&self, peer: SocketAddr) -> Option<BepId> {
        let peers = self.peers.read().await;

        match peers.get(&peer) {
            Some(PeerState::Pending { peer_id, .. }) => Some(peer_id.clone()),
            _ => None,
        }
    }

    pub async fn insert_pending(&self, addr: SocketAddr, peer_id: BepId, we_initiated: bool) {
        let mut peers = self.peers.write().await;

        peers.insert(
            addr,
            PeerState::Pending {
                peer_id,
                we_initiated,
            },
        );
    }

    pub async fn insert_pipeline(&self, addr: SocketAddr, peer_id: BepId, bitfield: Vec<u8>) {
        let mut peers = self.peers.write().await;

        let piece_count = self.metadata.piece_hashes().len();

        let caller = Node::from_socket(Key::new(&peer_id), addr);
        let pipeline = Pipeline::new(caller, BitField::from_wire(bitfield, piece_count));

        peers.insert(addr, PeerState::Active(Arc::new(pipeline)));
    }

    pub async fn activate_peer(&self, addr: SocketAddr, pipeline: Pipeline) {
        let mut peers = self.peers.write().await;
        peers.insert(addr, PeerState::Active(Arc::new(pipeline)));
    }

    pub async fn is_pending_outbound(&self, addr: &SocketAddr) -> bool {
        let peers = self.peers.read().await;
        matches!(
            peers.get(addr),
            Some(PeerState::Pending {
                we_initiated: true,
                ..
            })
        )
    }

    pub async fn broadcast_have(&self, index: u32) {
        let peers = self.peers.read().await;

        for (addr, state) in peers.iter() {
            if matches!(state, PeerState::Active(..)) {
                let _ = self
                    .bep_router
                    .send(*addr, StandardMessage::Have { index })
                    .await;
            }
        }
    }

    pub async fn broadcast(&self, message: StandardMessage) {
        let peers = self.peers.read().await;

        for (addr, state) in peers.iter() {
            if matches!(state, PeerState::Active(..)) {
                let _ = self.bep_router.send(*addr, message.clone()).await;
            }
        }
    }

    pub async fn terminate_with(&self, other: &SocketAddr) {
        let mut peers = self.peers.write().await;
        peers.remove(other);
    }

    pub async fn shutdown_peers(&self) {
        let peers = std::mem::take(&mut *self.peers.write().await);

        for (addr, _) in peers {
            self.bep_router.disconnect(&addr).await;
        }
    }

    pub fn piece_size(&self, index: u32) -> u32 {
        let total_pieces = self.metadata.piece_hashes().len() as u32;
        let piece_length = self.metadata.piece_length().value() as u32;
        let total_size = self.metadata.total_size();

        if index != total_pieces - 1 {
            return piece_length;
        }

        let remainder = (total_size % piece_length as u64) as u32;

        if remainder == 0 {
            return piece_length;
        }

        remainder
    }
}
