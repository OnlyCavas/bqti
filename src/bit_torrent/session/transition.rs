use std::{path::Path, sync::Arc};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::{
        chunks::{Downloading, MultiFileHandler, Seeding},
        torrent::metainfo::Metainfo,
    },
    session::{resume::ResumeFile, session::TorrentSession, state::TorrentState},
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
        handler: MultiFileHandler<Downloading>,
        resource: &Path,
        user_space_pwd: &Path,
    ) {
        let (tx, rx) = mpsc::channel(64);
        let token = CancellationToken::new();

        if let Some(resume) = ResumeFile::open(&resource, self.metadata.info_hash()).await {
            debug!("found, resume file ... loading progress ...");
            *self.bitfield.write().await = resume.get_bitfield();
        }

        self.swap_state(TorrentState::Downloading {
            handler: Some(handler),
            token: token.clone(),
            tx,
        })
        .await;

        self.spawn_downloader(resource, user_space_pwd, rx, token.clone());
    }

    pub async fn transition_seeding(self: &Arc<Self>, handler: MultiFileHandler<Seeding>) {
        let (tx, rx) = mpsc::channel(256);
        let token = CancellationToken::new();

        self.swap_state(TorrentState::Seeding {
            handler: Some(handler),
            token: token.clone(),
            tx,
        })
        .await;

        self.spawn_seeder(rx, token.clone());
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
