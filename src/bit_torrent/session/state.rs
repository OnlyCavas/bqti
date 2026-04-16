use std::path::PathBuf;

use tokio::{io, sync::mpsc};
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::chunks::{Downloading, MultiFileHandler, Reader, Seeding},
    hasher::Sha1Hash,
    save,
    session::{
        BitField, TorrentSessionError,
        resume::ResumeFile,
        session::{PieceRequest, TorrentSession},
    },
    torrent::metainfo::{InfoHash, Metainfo, TorrentFile},
    utils,
};

pub struct StateResources<Mode> {
    root: PathBuf,
    info_hash: InfoHash,
    pub handler: MultiFileHandler<Mode>,
}

impl<Mode> StateResources<Mode> {
    pub fn get_concrete_path(&self) -> PathBuf {
        self.root.clone()
    }

    pub async fn get_resume(&self) -> Option<ResumeFile> {
        let root = self.get_concrete_path();
        ResumeFile::open(&root, &self.info_hash).await
    }

    pub async fn persist_resume(&self, session: &TorrentSession) {
        let snapshot = session.get_bitfield().await;
        let data = ResumeFile::new(&self.info_hash, snapshot, &self.root);

        if let Err(e) = data.persist(&self.root).await {
            warn!("failed to persist resume data: {}", e);
        }
    }
}

impl StateResources<Downloading> {
    pub async fn download(metafile: &TorrentFile) -> Result<Self, TorrentSessionError> {
        let piece_length = metafile.piece_length();
        let files = metafile.files();
        let info_hash = metafile.info_hash();

        let Some(bqti_path) = utils::bqti::downloads_dir(info_hash.to_string()) else {
            return Err(TorrentSessionError::UnableToFindXDGFolder());
        };

        let handler = MultiFileHandler::download(&bqti_path, piece_length, files).await?;

        let state = Self {
            root: bqti_path,
            info_hash: info_hash.clone(),
            handler,
        };

        Ok(state)
    }
}

impl StateResources<Seeding> {
    pub async fn seed(
        user_space: PathBuf,
        metafile: &TorrentFile,
    ) -> Result<(Self, BitField), TorrentSessionError> {
        let piece_length = metafile.piece_length();
        let files = metafile.files();
        let info_hash = metafile.info_hash();

        let Some(uploads) = &utils::bqti::uploads_dir(&user_space, &metafile) else {
            return Err(TorrentSessionError::UnableToFindXDGFolder());
        };

        let handler = MultiFileHandler::seed(&uploads, piece_length, files).await?;
        let resume = ResumeFile::open(&uploads, metafile.info_hash()).await;
        let seed_metadata_path = uploads.join(".torrent");

        if !seed_metadata_path.exists() {
            match save(seed_metadata_path, &metafile) {
                Ok(_) => debug!("persist {} .torrent", metafile.info_hash().to_string()),
                Err(_) => error!("failed to persist .torrent"),
            }
        }

        let bitfield = match resume {
            Some(r) if r.is_complete() => r.get_bitfield(),
            _ => {
                let verified = Self::verify_pieces(&handler, &metafile.piece_hashes()).await?;

                let data = ResumeFile::new(info_hash, verified.clone(), &uploads);

                if let Err(e) = data.persist(&uploads).await {
                    warn!("Failed to persist resume: {}", e);
                }

                verified
            }
        };

        let state = Self {
            root: uploads.clone(),
            info_hash: metafile.info_hash().clone(),
            handler,
        };

        Ok((state, bitfield))
    }

    async fn verify_pieces(
        handler: &MultiFileHandler<Seeding>,
        piece_hashes: &[Vec<u8>],
    ) -> Result<BitField, TorrentSessionError> {
        let mut bitfield = BitField::empty(piece_hashes.len());

        for (index, expected) in piece_hashes.iter().enumerate() {
            let data = handler.read_piece(index as u32).await?;
            let piece_hash = *Sha1Hash::digest(&data).as_bytes();

            if piece_hash.as_slice() == expected.as_slice() {
                bitfield.set(index);
                info!("piece {} verified", index);
            } else {
                warn!("piece {} corrupted or missing", index);
            }
        }

        Ok(bitfield)
    }
}

pub enum TorrentState {
    Idle,
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

impl TorrentState {
    pub fn cancel_inner(&self) {
        match self {
            Self::Downloading { token, .. } => token.cancel(),
            Self::Seeding { token, .. } => token.cancel(),
            _ => {}
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
                info!("purging {:?}", path);
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
