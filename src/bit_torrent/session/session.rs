use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::{
    sync::{RwLock, mpsc, oneshot},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::{
        chunks::{PieceAssembler, Reader, Writer},
        torrent::metainfo::{Metainfo, PieceIntegrity, TorrentFile},
    },
    dht::{Key, Node},
    session::{
        BepId, StandardMessage, TorrentSessionError,
        bep::{BepRouter, PeerState, Pipeline},
        bit_field::BitField,
        state::TorrentState,
    },
};

pub enum SessionMode {
    Download { target_dir: PathBuf },
    Seed { source_dir: PathBuf },
}

impl SessionMode {
    pub fn dir(&self) -> &PathBuf {
        match self {
            SessionMode::Download { target_dir } => target_dir,
            SessionMode::Seed { source_dir } => source_dir,
        }
    }
}

pub struct TorrentSession {
    bep_router: Arc<BepRouter>,

    pub metadata: TorrentFile,
    pub(crate) state: RwLock<TorrentState>,
    pub(crate) bitfield: Arc<RwLock<BitField>>,

    peers: RwLock<HashMap<SocketAddr, PeerState>>,
    assemblers: RwLock<HashMap<u32, PieceAssembler>>,

    purge_on_drop: AtomicBool,
}

pub struct PieceRequest {
    pub index: u32,
    pub begin: u32,
    pub length: u32,
    pub respond: oneshot::Sender<Vec<u8>>,
}

impl TorrentSession {
    pub async fn new(
        mode: SessionMode,
        metadata: TorrentFile,
        bep_router: Arc<BepRouter>,
    ) -> Result<Arc<Self>, TorrentSessionError> {
        let piece_count = metadata.piece_hashes().len();

        let session = Arc::new(Self {
            metadata: metadata.clone(),
            state: RwLock::new(TorrentState::Idle),
            bitfield: Arc::new(RwLock::new(BitField::empty(piece_count))),
            bep_router: bep_router.clone(),
            peers: RwLock::new(HashMap::new()),
            assemblers: RwLock::new(HashMap::new()),
            purge_on_drop: AtomicBool::new(false),
        });

        match mode {
            SessionMode::Seed { source_dir } => {
                debug!("files found, loading pieces...");
                session.transition_seeding(&metadata, &source_dir).await?;
            }
            SessionMode::Download { target_dir } => {
                info!("no files found, starting download");
                session
                    .transition_downloading(&metadata, &target_dir)
                    .await?;
            }
        };

        bep_router.start_peer_discovery(session.clone());

        Ok(session)
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

            let piece_count = session.metadata.piece_hashes().len() - 1;

            loop {
                tokio::select! {
                    biased;
                    _ = cancellation_token.cancelled() => {
                        return;
                    },
                    message = downloader.recv() => match message {
                        None => {
                            info!("downloader is closed");
                            break;
                        },
                        Some((index, data)) => {
                            if !session.metadata.verify_hash(index, &data).is_ok() {
                                continue;
                            }

                            match resources.handler.write_piece(index, data).await {
                                Ok(_) => {
                                    info!("piece {} persisted", index);
                                    resources.persist_resume(&session).await;
                                },
                                Err(e) => warn!("failed to persist piece {}: {}", index, e),
                            }


                            if index == piece_count as u32 {
                                break;
                            }
                        }
                    }
                }
            }

            debug!("finish downloading, trying to switch to seeding...");

            // TODO needs to be at the client
            // let info_hash = session.metadata.info_hash();
            //
            // match utils::bqti::link(user_space, info_hash.to_string()).await {
            //     Ok(_) => info!("check, download/"),
            //     Err(e) => error!("fuck, {}", e),
            // }

            match session
                .transition_seeding(&session.metadata, &user_space)
                .await
            {
                Ok(_) => (),
                Err(_) => {
                    error!("failed to transition to seeding");
                    return;
                }
            };
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

            let mut state = session.state.write().await;
            let Some(resources) = state.take_seeding() else {
                return;
            };
            drop(state);

            debug!("started seeding...");

            loop {
                tokio::select! {
                    biased;
                    _ = cancellation_token.cancelled() => {
                        return;
                    },
                    message = uploader.recv() => match message {
                        None => {
                            info!("downloader is closed");
                            break;
                        },
                        Some(PieceRequest { index, begin, length, respond }) => {
                             match resources.handler.read_piece(index).await {
                                Ok(data) => {
                                    let begin = begin as usize;
                                    let end = (begin + length as usize).min(data.len());

                                    let _ = respond.send(data[begin..end].to_vec());
                                }
                                Err(e) => warn!("failed to read piece {}: {}", index, e),
                            }
                        }
                    }
                }
            }

            debug!("stoped seeding, state: idle");
            session.transition_idle().await
        });

        Ok(())
    }

    pub async fn send_downloaded_piece(&self, index: u32, data: Vec<u8>) {
        let state = self.state.read().await;

        let Some(tx) = state.downloader_tx() else {
            return;
        };

        if tx.send((index, data)).await.is_err() {
            warn!("failed to send to downloader, channel closed");
            return;
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

                let handshake_fut = session.bep_router.handshake(session.clone(), addr);

                if let Ok(result) =
                    tokio::time::timeout(std::time::Duration::from_secs(5), handshake_fut).await
                {
                    match result {
                        Ok(_) => debug!("bep handshake, connected: {}", addr),
                        Err(e) => debug!("bep handshake, failed: {} - {}", addr, e),
                    }
                } else {
                    debug!("bep handshake timeout: {}", addr);
                }
            });
        }
    }
}

impl TorrentSession {
    pub fn purge(&self) {
        self.purge_on_drop.store(true, Ordering::Relaxed);
    }

    pub async fn get_pipeline(&self, src: SocketAddr) -> Option<Arc<Pipeline>> {
        let peers = self.peers.read().await;

        match peers.get(&src) {
            Some(PeerState::Active(p)) => Some(p.clone()),
            Some(another) => {
                error!("fuck, {:?}", another);
                None
            }
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

    pub async fn has_block(&self, index: u32, begin: u32) -> bool {
        let assemblers = self.assemblers.read().await;

        match assemblers.get(&index) {
            Some(a) => a.has_block(begin),
            None => self.bitfield.read().await.have(index as usize),
        }
    }

    pub async fn add_block(&self, index: u32, begin: u32, data: Vec<u8>) -> Option<Vec<u8>> {
        let mut assemblers = self.assemblers.write().await;

        {
            let bitfield = self.bitfield.read().await;

            if bitfield.have(index as usize) {
                return None;
            }
        }

        let assembler = assemblers
            .entry(index)
            .or_insert_with(|| PieceAssembler::new(index, self.piece_size(index)));

        if assembler.add_block(begin, &data) {
            let Some(piece) = assemblers.remove(&index) else {
                return None;
            };

            {
                let mut bitfield = self.bitfield.write().await;
                bitfield.set(index as usize);
            }

            return Some(piece.assemble());
        }

        None
    }

    pub async fn get_pending_peer(&self, peer: SocketAddr) -> Option<BepId> {
        let peers = self.peers.read().await;

        match peers.get(&peer) {
            Some(PeerState::Pending { peer_id, .. }) => Some(peer_id.clone()),
            _ => None,
        }
    }

    pub async fn check_already_active(&self, peer: SocketAddr) {
        let mut peers = self.peers.write().await;

        if matches!(peers.get(&peer), Some(PeerState::Active(..))) {
            peers.remove(&peer);
        }
    }

    pub async fn insert_pending(&self, addr: SocketAddr, peer_id: BepId, we_initiated: bool) {
        let mut peers = self.peers.write().await;

        peers.entry(addr).or_insert(PeerState::Pending {
            peer_id,
            initiated: Instant::now(),
            we_initiated,
        });
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

    pub async fn remove_peer(&self, addr: &SocketAddr) {
        self.peers.write().await.remove(addr);
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

    pub async fn shutdown_peers(&self) {
        self.peers.write().await.clear();
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
