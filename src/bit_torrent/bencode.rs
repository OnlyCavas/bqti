use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::bit_torrent::{EncodedBytes, Hash2OBytes, torrent::Torrent};

#[derive(Error, Debug)]
pub enum BencodeError {
    #[error("Failed to read from file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Decode error: {0}")]
    Decode(String),
}

pub fn decode(path: &str) -> Result<Torrent, BencodeError> {
    let bytes = std::fs::read(path).map_err(BencodeError::Io)?;
    serde_bencode::from_bytes::<Torrent>(&bytes).map_err(|e| BencodeError::Decode(e.to_string()))
}

pub fn info_hash(torrent: &Torrent) -> Result<Hash2OBytes, BencodeError> {
    let info_bytes =
        serde_bencode::to_bytes(&torrent.info).map_err(|e| BencodeError::Decode(e.to_string()))?;

    Ok(Sha1::digest(info_bytes).into())
}

pub fn encode(torrent: &Torrent) -> Result<EncodedBytes, BencodeError> {
    match serde_bencode::to_bytes(&torrent) {
        Ok(bytes) => Ok(bytes),
        Err(e) => Err(BencodeError::Decode(e.to_string())),
    }
}
