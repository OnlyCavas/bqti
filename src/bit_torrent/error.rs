use thiserror::Error;

use crate::bit_torrent::torrent::{codec::BencodeError, metainfo::TorrentError};

#[derive(Error, Debug)]
pub enum BitTorrentError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("codec error: {0}")]
    Codec(#[from] BencodeError),

    #[error("path invalid or not found")]
    InvalidPath(),

    #[error("torrent error: {0}")]
    Torrent(#[from] TorrentError),
}
