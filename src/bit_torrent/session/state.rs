use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::chunks::{Downloading, MultiFileHandler, Seeding},
    session::session::PieceRequest,
};

pub enum TorrentState {
    Idle,
    Downloading {
        handler: Option<MultiFileHandler<Downloading>>,
        token: CancellationToken,
        tx: mpsc::Sender<(u32, Vec<u8>)>,
    },
    Seeding {
        handler: Option<MultiFileHandler<Seeding>>,
        token: CancellationToken,
        tx: mpsc::Sender<PieceRequest>,
    },
    Cancelled,
}

impl TorrentState {
    fn cancel_inner(&self) {
        match self {
            Self::Downloading { token, .. } => token.cancel(),
            Self::Seeding { token, .. } => token.cancel(),
            _ => {}
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

    pub fn take_downloading_handler(&mut self) -> Option<MultiFileHandler<Downloading>> {
        match self {
            Self::Downloading { handler, .. } => handler.take(),
            _ => None,
        }
    }

    pub fn take_seeder_handler(&mut self) -> Option<MultiFileHandler<Seeding>> {
        match self {
            Self::Seeding { handler, .. } => handler.take(),
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
