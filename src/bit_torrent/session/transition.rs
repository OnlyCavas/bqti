use std::{path::Path, sync::Arc};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::chunks::{Downloading, Seeding},
    session::{
        TorrentSessionError,
        session::TorrentSession,
        state::{StateResources, TorrentState},
    },
    torrent::metainfo::TorrentFile,
};

impl TorrentSession {
    async fn swap_state(&self, next: TorrentState) {
        let mut guard = self.state.write().await;

        if guard.is_cancelled() {
            return;
        }

        *guard = next;
    }

    pub async fn transition_downloading(
        self: &Arc<Self>,
        metafile: &TorrentFile,
        user_space: &Path,
    ) -> Result<(), TorrentSessionError> {
        let (tx, rx) = mpsc::channel(64);
        let token = CancellationToken::new();
        let resources = StateResources::<Downloading>::download(&metafile).await?;

        let path = resources.get_concrete_path();
        if let Some(resume) = resources.get_resume().await {
            debug!("found, resume file ... loading progress ...");
            *self.bitfield.write().await = resume.get_bitfield();
        }

        self.swap_state(TorrentState::Downloading {
            resources: Some(resources),
            token: token.clone(),
            tx,
            path,
        })
        .await;

        self.spawn_downloader(user_space, rx, token.clone())
    }

    pub async fn transition_seeding(
        self: &Arc<Self>,
        metafile: &TorrentFile,
        user_space: &Path,
    ) -> Result<(), TorrentSessionError> {
        let (tx, rx) = mpsc::channel(256);
        let token = CancellationToken::new();

        let (resources, bitfield) =
            StateResources::<Seeding>::seed(user_space.into(), metafile).await?;

        let path = resources.get_concrete_path();
        *self.bitfield.write().await = bitfield;

        self.swap_state(TorrentState::Seeding {
            token: token.clone(),
            tx,
            resources: Some(resources),
            path,
        })
        .await;

        self.spawn_seeder(rx, token.clone())
    }

    pub async fn transition_idle(self: &Arc<Self>) {
        self.swap_state(TorrentState::Idle).await;
    }

    pub async fn transition_cancelled(self: &Arc<Self>) {
        let is_cancelled = {
            let state = self.state.read().await;
            state.is_cancelled()
        };

        if !is_cancelled {
            self.swap_state(TorrentState::Cancelled).await;
        }
    }
}
