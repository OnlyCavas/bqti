use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Torrent;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IpcState {
    pub active_torrents: HashMap<String, Torrent>,
}
