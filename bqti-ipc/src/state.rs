use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{Event, Torrent};

pub trait EventStream {
    fn replicate(&self) -> Vec<Event>;
    fn apply(&mut self, event: Event) -> Option<Event>;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IpcState {
    pub active_torrents: HashMap<String, Torrent>,
}

impl EventStream for IpcState {
    fn replicate(&self) -> Vec<Event> {
        self.active_torrents
            .values()
            .map(|torrent| Event::SessionStateChanged {
                info_hash: torrent.info_hash.clone(),
                name: torrent.name.clone(),
                state: torrent.state.clone(),
            })
            .collect()
    }

    fn apply(&mut self, event: Event) -> Option<Event> {
        match event {
            Event::SessionStateChanged {
                ref info_hash,
                ref name,
                ref state,
            } => {
                if let Some(torrent) = self.active_torrents.get_mut(info_hash) {
                    torrent.name = name.clone();
                    torrent.state = state.clone();
                }

                Some(event)
            }
            Event::TorrentAdded {
                ref info_hash,
                ref name,
                ref state,
            } => {
                self.active_torrents.insert(
                    info_hash.clone(),
                    Torrent {
                        info_hash: info_hash.clone(),
                        name: name.clone(),
                        state: state.clone(),
                    },
                );

                Some(event)
            }
            Event::TorrentRemoved { ref info_hash } => {
                self.active_torrents.remove(info_hash);

                Some(event)
            }
            _ => Some(event),
        }
    }
}
