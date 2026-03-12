use thiserror::Error;

use crate::bit_torrent::{
    torrent::codec::{Metadata, MetadataInfo},
    types::EncodedBytes,
};

#[derive(Error, Debug)]
pub enum BencodeError {
    #[error("Failed to read from file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Decode error: {0}")]
    Decode(String),
}

pub fn decode(bytes: Vec<u8>) -> Result<Metadata, BencodeError> {
    let metadata = serde_bencode::from_bytes::<Metadata>(&bytes)
        .map_err(|e| BencodeError::Decode(e.to_string() + "here"))?;

    Ok(metadata)
}

pub fn info_hash(meta_info: &MetadataInfo) -> Result<EncodedBytes, BencodeError> {
    match serde_bencode::to_bytes(&meta_info) {
        Ok(bytes) => Ok(bytes),
        Err(e) => Err(BencodeError::Decode(e.to_string())),
    }
}

pub fn encode(metadata: &Metadata) -> Result<EncodedBytes, BencodeError> {
    match serde_bencode::to_bytes(&metadata) {
        Ok(bytes) => Ok(bytes),
        Err(e) => Err(BencodeError::Decode(e.to_string())),
    }
}
