use thiserror::Error;

use crate::bit_torrent::{
    EncodedBytes,
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
        .map_err(|e| BencodeError::Decode(e.to_string() + "here"))?;

    let torrent = TorrentBuilder::from_metadata(metadata)
        .map_err(|e| BencodeError::Decode(e.to_string() + "nope"))?;

    Ok(torrent)
}

pub fn encode(torrent: &Metadata) -> Result<EncodedBytes, BencodeError> {
    match serde_bencode::to_bytes(&torrent) {
        Ok(bytes) => Ok(bytes),
        Err(e) => Err(BencodeError::Decode(e.to_string())),
    }
}
