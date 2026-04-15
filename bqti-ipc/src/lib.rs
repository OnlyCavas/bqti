use core::fmt;
use std::{
    env,
    fmt::{Display, Formatter},
    io,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

mod socket;

pub use socket::Socket;

use crate::socket::SOCKET_PATH;
pub type Reply = Result<Response, String>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Request {
    Status,
    // Add a torrent to queue: path or magnet
    AddDownload { source: String },
    AddSeed { source: String },
    // Remove a torrent
    RemoveTorrent { info_hash: String },
    Torrents,
    Shutdown,
    EventStream, // start a event stream of concorrent events
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "payload")]
pub enum Response {
    Handled,
    TorrentAdded { info_hash: String }, // Add Torrent Response
    Status(DaemonStatus),
    Torrents(Vec<TorrentInfo>),
    Torrent(TorrentInfo),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Event {
    DownloadStarted {
        info_hash: String,
        name: String,
    },
    DownloadComplete {
        info_hash: String,
    },
    SeedStarted {
        info_hash: String,
    },
    PieceDownloaded {
        info_hash: String,
        index: u32,
    },
    StateChanged {
        info_hash: String,
        state: TorrentState,
    },
    Error {
        info_hash: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TorrentState {
    Idle,
    Downloading,
    Seeding,
    Stopped,
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TorrentInfo {
    pub info_hash: String,
}

pub fn socket_path() -> io::Result<PathBuf> {
    env::var_os(SOCKET_PATH).map(PathBuf::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{SOCKET_PATH} is not set, is the bqti daemon running?"),
        )
    })
}
