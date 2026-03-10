use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::bit_torrent::{
    ByteSize, EncodedBytes, Hash2OBytes,
    torrent::{builder::TorrentBuilder, reader::Metadata, torrent::TorrentFile},
};

#[derive(Error, Debug)]
pub enum BencodeError {
    #[error("Failed to read from file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Decode error: {0}")]
    Decode(String),
}

pub fn decode(path: &str) -> Result<TorrentFile, BencodeError> {
    let bytes = std::fs::read(path).map_err(BencodeError::Io)?;
    let metadata = serde_bencode::from_bytes::<Metadata>(&bytes)
        .map_err(|e| BencodeError::Decode(e.to_string()))?;

    let torrent =
        TorrentBuilder::from_metadata(metadata).map_err(|e| BencodeError::Decode(e.to_string()))?;

    Ok(torrent)
}

pub fn info_hash(torrent: &Metadata) -> Result<Hash2OBytes, BencodeError> {
    let info_bytes =
        serde_bencode::to_bytes(&torrent.info).map_err(|e| BencodeError::Decode(e.to_string()))?;

    Ok(Sha1::digest(info_bytes).into())
}

pub fn encode(torrent: &Metadata) -> Result<EncodedBytes, BencodeError> {
    match serde_bencode::to_bytes(&torrent) {
        Ok(bytes) => Ok(bytes),
        Err(e) => Err(BencodeError::Decode(e.to_string())),
    }
}

#[derive(Debug)]
pub struct FileInfo {
    pub length: ByteSize,       // size of each file
    pub path: Vec<String>,      // path
    pub md5sum: Option<String>, // file md5 sum
}
