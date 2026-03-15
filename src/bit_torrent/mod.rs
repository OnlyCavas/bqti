use crate::bit_torrent::{
    bencode::{BencodeTorrent, decode, encode},
    torrent::{builder::TorrentBuilder, metainfo::TorrentFile},
};

mod bencode;
mod error;
pub mod torrent;
pub mod types;

pub fn load(path: &str) -> Result<TorrentFile, BitTorrentError> {
    let bytes = std::fs::read(path).map_err(BitTorrentError::Io)?;
    let info = decode::<BencodeTorrent>(&bytes).map_err(BitTorrentError::Codec)?;
    let torrent = TorrentBuilder::apply_from_metadata(info).map_err(BitTorrentError::Torrent)?;

    Ok(torrent)
}

pub fn save(path: &str, torrent: &TorrentFile) -> Result<(), BitTorrentError> {
    let bencode_data = BencodeTorrent::from(torrent);
    let bytes = encode(&bencode_data).map_err(BitTorrentError::Codec)?;

    std::fs::write(path, bytes)?;

    Ok(())
}

pub use error::BitTorrentError;
