use crate::{
    bit_torrent::torrent::metainfo::{InfoHash, TorrentFile},
    network::ConnectionManager,
};

pub struct TorrentSession {
    pub info_hash: InfoHash,
    pub manager: ConnectionManager,
}

impl TorrentSession {
    fn from_torrent(_torrent: TorrentFile) {}
}
