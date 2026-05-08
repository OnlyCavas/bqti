use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::{io, sync::mpsc};
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::chunks::{Downloading, MultiFileHandler, Reader, Seeding},
    hasher::Sha1Hash,
    session::{
        BitField, TorrentSessionError,
        cache::{CachingMode, SessionCache},
        manager::SessionEvent,
        resume::ResumeFile,
        session::{PieceRequest, TorrentSession},
        transition::Transition,
    },
    torrent::metainfo::{Metainfo, TorrentFile},
};

pub struct StateResources<Mode> {
    pub(crate) handler: Arc<MultiFileHandler<Mode>>,
    cache: SessionCache,
}

impl<Mode> StateResources<Mode> {
    pub async fn persist_resume(&self, session: &TorrentSession) {
        let snapshot = session.get_bitfield().await;
        self.cache.persist_resume(snapshot).await;
    }

    pub fn get_resume(&self) -> Option<&ResumeFile> {
        self.cache.resume.as_ref()
    }

    pub fn get_root_path(&self) -> &Path {
        &self.cache.dir
    }
}

impl StateResources<Downloading> {
    pub async fn download(metafile: Arc<TorrentFile>) -> Result<Self, TorrentSessionError> {
        let piece_length = metafile.piece_length();
        let files = metafile.files();

        let cache = match SessionCache::new(CachingMode::Download {
            metafile: metafile.clone(),
        })
        .await
        {
            Some(cache) => cache,
            None => return Err(TorrentSessionError::UnableToFindXDGFolder()),
        };

        let handler = MultiFileHandler::download(&cache.dir, piece_length, files).await?;
        cache.persist_torrent().await;

        let state = Self {
            handler: Arc::new(handler),
            cache,
        };

        Ok(state)
    }
}

impl StateResources<Seeding> {
    pub async fn seed(
        user_space: PathBuf,
        metafile: Arc<TorrentFile>,
        event_tx: &mpsc::Sender<SessionEvent>,
    ) -> Result<(Self, BitField), TorrentSessionError> {
        let piece_length = metafile.piece_length();
        let files = metafile.files();

        let cache = match SessionCache::new(CachingMode::Seed {
            user_space,
            metafile: metafile.clone(),
        })
        .await
        {
            Some(cache) => cache,
            None => return Err(TorrentSessionError::UnableToFindXDGFolder()),
        };

        let handler = Arc::new(MultiFileHandler::seed(&cache.dir, piece_length, files).await?);
        cache.persist_torrent().await;

        let bitfield = match &cache.resume {
            Some(resume) if resume.is_complete() => resume.get_bitfield(),
            _ => {
                let verified =
                    Self::verify_pieces(&handler, &metafile.piece_hashes(), &event_tx).await?;

                cache.persist_resume(verified.clone()).await;

                verified
            }
        };

        Ok((Self { handler, cache }, bitfield))
    }

    async fn verify_pieces(
        handler: &MultiFileHandler<Seeding>,
        piece_hashes: &[Vec<u8>],
        event_tx: &mpsc::Sender<SessionEvent>,
    ) -> Result<BitField, TorrentSessionError> {
        let total_pieces = piece_hashes.len();
        let mut bitfield = BitField::empty(total_pieces);

        for (index, expected) in piece_hashes.iter().enumerate() {
            let data = handler.read_piece(index as u32).await?;
            let piece_hash = *Sha1Hash::digest(&data).as_bytes();

            if piece_hash.as_slice() != expected.as_slice() {
                continue;
            }

            bitfield.set(index);

            if let Err(_) = event_tx
                .send(SessionEvent::PieceVerified {
                    total_pieces: total_pieces as u32,
                    current: index as u32,
                })
                .await
            {
                warn!("failed to send verifying piece event");
            }
        }

        Ok(bitfield)
    }
}

#[derive(Clone)]
pub enum LastState {
    Downloading,
    Seeding { path: PathBuf },
}

pub enum TorrentState {
    Idle(Option<LastState>),
    Downloading {
        path: PathBuf,
        resources: Option<StateResources<Downloading>>,
        token: CancellationToken,
        tx: mpsc::Sender<(u32, Vec<u8>)>,
    },
    Seeding {
        path: PathBuf,
        resources: Option<StateResources<Seeding>>,
        token: CancellationToken,
        tx: mpsc::Sender<PieceRequest>,
    },
    Cancelled,
}

#[derive(Debug)]
pub enum ActiveMode {
    Downloading,
    Seeding,
    Idle,
}

impl Default for TorrentState {
    fn default() -> Self {
        TorrentState::Idle(None)
    }
}

impl TorrentSession {
    pub async fn current_mode(&self) -> ActiveMode {
        match &*self.state.read().await {
            TorrentState::Downloading { .. } => ActiveMode::Downloading,
            TorrentState::Seeding { .. } => ActiveMode::Seeding,
            _ => ActiveMode::Idle,
        }
    }
}

impl TorrentState {
    pub(crate) fn cancel_inner(&mut self) {
        match self {
            Self::Downloading { token, .. } => {
                token.cancel();
            }
            Self::Seeding { token, .. } => {
                token.cancel();
            }
            _ => {}
        }
    }

    pub fn can_transition_to(&self, to: &Transition) -> bool {
        match (self, to) {
            (TorrentState::Idle(None), Transition::Download { .. })
            | (TorrentState::Idle(None), Transition::Seed { .. })
            | (TorrentState::Downloading { .. }, Transition::Pause)
            | (TorrentState::Downloading { .. }, Transition::Cancel)
            | (TorrentState::Downloading { .. }, Transition::Seed { .. })
            | (TorrentState::Seeding { .. }, Transition::Pause)
            | (TorrentState::Seeding { .. }, Transition::Cancel)
            | (TorrentState::Idle(Some(_)), Transition::Resume { .. })
            | (TorrentState::Idle(Some(_)), Transition::Cancel) => true,
            _ => false,
        }
    }

    pub fn purge(&mut self) -> impl Future<Output = io::Result<()>> + 'static {
        let path = match self {
            TorrentState::Downloading { path, .. } => Some(path.clone()),
            TorrentState::Seeding { path, .. } => Some(path.clone()),
            _ => None,
        };

        async move {
            if let Some(path) = path {
                tokio::fs::remove_dir_all(path).await
            } else {
                Ok(())
            }
        }
    }

    pub fn downloader_tx(&self) -> Option<&mpsc::Sender<(u32, Vec<u8>)>> {
        match self {
            Self::Downloading { tx, .. } => Some(tx),
            _ => None,
        }
    }

    pub fn uploader_tx(&self) -> Option<&mpsc::Sender<PieceRequest>> {
        match self {
            Self::Seeding { tx, .. } => Some(tx),
            _ => None,
        }
    }

    pub fn take_downloading(&mut self) -> Option<StateResources<Downloading>> {
        match self {
            TorrentState::Downloading { resources, .. } => resources.take(),
            _ => None,
        }
    }

    pub fn take_seeding(&mut self) -> Option<StateResources<Seeding>> {
        match self {
            TorrentState::Seeding { resources, .. } => resources.take(),
            _ => None,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl Drop for TorrentState {
    fn drop(&mut self) {
        self.cancel_inner();
    }
}
