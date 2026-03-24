use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::types::EncodedBytes;

#[derive(Error, Debug)]
pub enum BencodeError {
    #[error("Failed to read from file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Decode error: {0}")]
    Decode(String),
}

pub fn decode<TData: DeserializeOwned>(bytes: &[u8]) -> Result<TData, BencodeError> {
    let metadata = serde_bencode::from_bytes::<TData>(&bytes)
        .map_err(|e| BencodeError::Decode(e.to_string()))?;

    Ok(metadata)
}

pub fn info_hash<TData: Serialize>(data: TData) -> Result<EncodedBytes, BencodeError> {
    match serde_bencode::to_bytes(&data) {
        Ok(bytes) => Ok(bytes),
        Err(e) => Err(BencodeError::Decode(e.to_string())),
    }
}

pub fn encode<TData: Serialize>(value: &TData) -> Result<EncodedBytes, BencodeError> {
    match serde_bencode::to_bytes(&value) {
        Ok(bytes) => Ok(bytes),
        Err(e) => Err(BencodeError::Decode(e.to_string())),
    }
}
