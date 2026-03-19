use crate::bit_torrent::{network::session::PeerSession, torrent::metainfo::InfoHash};

pub struct TorrentSession {
    pub info_hash: InfoHash,
    pub peers: Vec<PeerSession>,
}
