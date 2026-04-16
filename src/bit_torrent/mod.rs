use std::path::Path;

use crate::bit_torrent::{
    bencode::{BencodeTorrent, decode, encode},
    torrent::{builder::TorrentBuilder, metainfo::TorrentFile},
};

mod bencode;
mod bqti;
pub mod certs;
mod chunks;
pub mod dht;
mod error;
pub mod hasher;
mod magnet;

pub mod network;
mod pex;
pub mod session;
pub mod torrent;
pub mod types;

pub fn load(path: impl AsRef<Path>) -> Result<TorrentFile, BitTorrentError> {
    let bytes = std::fs::read(path).map_err(BitTorrentError::Io)?;
    let info = decode::<BencodeTorrent>(&bytes).map_err(BitTorrentError::Codec)?;
    let torrent = TorrentBuilder::apply_from_metadata(info).map_err(BitTorrentError::Torrent)?;

    Ok(torrent)
}

pub fn save(path: impl AsRef<Path>, torrent: &TorrentFile) -> Result<(), BitTorrentError> {
    let bencode_data = BencodeTorrent::from(torrent);
    let bytes = encode(&bencode_data).map_err(BitTorrentError::Codec)?;

    std::fs::write(path, bytes)?;

    Ok(())
}

pub use bqti::{Bqti, SeedingOptions, TorrentAction, TorrentSource, Torrenting};
pub use error::BitTorrentError;
