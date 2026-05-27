use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use strum::Display;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::chunks::{Downloading, Seeding},
    session::{
        TorrentSessionError,
        manager::SessionEvent,
        session::TorrentSession,
        state::{LastState, StateResources, TorrentState},
    },
    torrent::metainfo::TorrentFile,
};

#[derive(Display)]
pub enum Transition {
    Download {
        metafile: Arc<TorrentFile>,
        dest: PathBuf,
    },
    Seed {
        path: PathBuf,
        metafile: Arc<TorrentFile>,
    },
    Pause,
    Resume {
        metafile: Arc<TorrentFile>,
    },
    Cancel,
}

impl TorrentSession {
    async fn swap_state(&self, next: TorrentState) {
        let mut guard = self.state.write().await;

        if guard.is_cancelled() {
            return;
        }

        *guard = next;
    }

    pub async fn transition(self: &Arc<Self>, to: Transition) -> Result<(), TorrentSessionError> {
        {
            let state = self.state.read().await;

            if !state.can_transition_to(&to) {
                return Err(TorrentSessionError::InvalidTransition(to.to_string()));
            }
        }

        {
            let mut state = self.state.write().await;
            state.cancel_inner();
        }

        match to {
            Transition::Download { metafile, dest } => {
                self.transition_downloading(metafile, dest).await
            }
            Transition::Seed { path, metafile } => self.transition_seeding(metafile, &path).await,
            Transition::Pause => {
                self.transition_idle().await;
                Ok(())
            }
            Transition::Resume { metafile } => {
                let paused_from: LastState = {
                    let state = self.state.read().await;

                    match &*state {
                        TorrentState::Idle(Some(last)) => last.clone(),
                        _ => unreachable!("guard already checked"),
                    }
                };

                match paused_from {
                    LastState::Downloading { dest } => {
                        self.transition_downloading(metafile, dest).await
                    }
                    LastState::Seeding { path } => self.transition_seeding(metafile, &path).await,
                }
            }
            Transition::Cancel => {
                self.transition_cancelled().await;
                Ok(())
            }
        }
    }

    async fn transition_downloading(
        self: &Arc<Self>,
        metafile: Arc<TorrentFile>,
        dest: PathBuf,
    ) -> Result<(), TorrentSessionError> {
        let (tx, rx) = mpsc::channel(64);

        let token = CancellationToken::new();
        let resources = StateResources::<Downloading>::download(metafile.clone(), &dest).await?;

        let path = resources.get_root_path().to_path_buf();

        if let Some(resume) = resources.get_resume() {
            debug!("loading cache .bqtiresume");
            *self.bitfield.write().await = resume.get_bitfield();
        }

        self.swap_state(TorrentState::Downloading {
            resources: Some(resources),
            token: token.clone(),
            tx,
            path: path.clone().into(),
            dest,
        })
        .await;

        self.spawn_downloader(&path, rx, token.clone())?;
        Ok(())
    }

    async fn transition_seeding(
        self: &Arc<Self>,
        metafile: Arc<TorrentFile>,
        user_space: &Path,
    ) -> Result<(), TorrentSessionError> {
        let (tx, rx) = mpsc::channel(256);
        let token = CancellationToken::new();

        let (resources, bitfield) =
            StateResources::<Seeding>::seed(user_space.into(), metafile, &self.event_tx).await?;

        let path = resources.get_root_path().to_path_buf();
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

    async fn transition_idle(self: &Arc<Self>) {
        let last = {
            let state = self.state.read().await;
            match &*state {
                TorrentState::Downloading { dest, .. } => {
                    Some(LastState::Downloading { dest: dest.clone() })
                }
                TorrentState::Seeding { path, .. } => {
                    Some(LastState::Seeding { path: path.clone() })
                }
                _ => None,
            }
        };

        self.swap_state(TorrentState::Idle(last)).await;
        self.send_event(SessionEvent::Idle).await;
    }

    async fn transition_cancelled(self: &Arc<Self>) {
        let is_cancelled = {
            let state = self.state.read().await;
            state.is_cancelled()
        };

        if !is_cancelled {
            self.swap_state(TorrentState::Cancelled).await;
        }
    }
}
