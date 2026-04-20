use core::fmt;
use std::{
    fmt::{Display, Formatter},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

mod socket;
pub mod state;

pub use socket::Socket;

use crate::socket::SOCKET_PATH;
pub type Reply = Result<Response, String>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Request {
    Status {
        info_hash: Option<String>,
    },
    AddDownload {
        link: String,
    },
    AddSeed {
        name: Option<String>,
        path: String,
        piece_length: u64,
        announce: Vec<Vec<String>>,
        seeds: Option<Vec<String>>,
        nodes: Option<Vec<String>>,
        private: bool,
        comment: Option<String>,
        created_by: Option<String>,
    },
    PauseSession {
        info_hash: String,
    },
    ResumeSession {
        info_hash: String,
    },
    RemoveTorrent {
        info_hash: String,
    },
    Torrents,
    Shutdown,
    EventStream,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "payload")]
pub enum Response {
    Handled,
    TorrentAdded { info_hash: String },
    SeedAdded { info_hash: String },
    Removed { info_hash: String },
    Status(DaemonStatus),
    Torrents(Vec<Torrent>),
    Torrent(Torrent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Event {
    ExposeTorrent {
        info_hash: String,
        magnet: String,
    },

    SessionStateChanged {
        info_hash: String,
        name: String,
        state: TorrentState,
    },

    DownloadComplted {
        info_hash: String,
        resource_path: String,
    },

    TorrentAdded {
        info_hash: String,
        name: String,
        state: TorrentState,
    },

    TorrentRemoved {
        info_hash: String,
    },

    Error {
        info_hash: String,
        message: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub enum TorrentState {
    #[default]
    Pending,
    Paused,
    Downloading {
        current: u32,
        total_pieces: u32,
        download_rate: u64,
    },
    Verifying {
        verified: u32,
        total: u32,
    },
    Seeding {
        upload_rate: u64,
        peers: u32,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Torrent {
    pub info_hash: String,
    pub name: String,
    pub state: TorrentState,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonStatus {
    pub version: String,
    pub active_torrents: usize,
    pub upload_rate: u64,
    pub download_rate: u64,
}

impl Display for DaemonStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "bqti daemon v{}", self.version)?;
        writeln!(f, "  torrents : {}", self.active_torrents)?;
        writeln!(f, "  upload   : {}/s", fmt_bytes(self.upload_rate))?;
        writeln!(f, "  download : {}/s", fmt_bytes(self.download_rate))
    }
}

fn fmt_bytes(bytes: u64) -> String {
    match bytes {
        0 => "0 B".to_string(),
        b if b < 1_024 => format!("{} B", b),
        b if b < 1_048_576 => format!("{:.1} KB", b as f64 / 1_024.0),
        b if b < 1_073_741_824 => format!("{:.1} MB", b as f64 / 1_048_576.0),
        b => format!("{:.1} GB", b as f64 / 1_073_741_824.0),
    }
}

pub fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os(SOCKET_PATH) {
        return PathBuf::from(path);
    }

    let instance = std::env::var("BQTI_INSTANCE").unwrap_or_else(|_| "default".into());
    let uid = unsafe { libc::getuid() };

    format!("/run/user/{uid}/bqti/{instance}.sock").into()
}
