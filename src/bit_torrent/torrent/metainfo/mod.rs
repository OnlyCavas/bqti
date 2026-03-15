use enum_dispatch::enum_dispatch;
use thiserror::Error;

use crate::{
    bit_torrent::torrent::metainfo::{
        v1::{EmbededFile, TorrentV1},
        v2::TorrentV2,
    },
    types::{ByteSize, Hash2OBytes, Hash32Bytes, UnixDate},
};

pub mod v1;
pub mod v2;

#[derive(Error, Debug)]
pub enum TorrentError {
    #[error("piece hash doesn't have 20 bytes")]
    Hash20Error(),

    #[error("failed: {0}")]
    Failed(String),

    #[error("validation failed: {0}")]
    NotValid(String),

    #[error("unsupported version, {0}")]
    UnsupportedVersion(u8),

    #[error("unsupported, {0}")]
    Unsupported(String),
}

#[enum_dispatch(Metainfo, Integrity)]
pub enum TorrentFile {
    V1(TorrentV1),
    V2(TorrentV2),
}

#[enum_dispatch]
pub trait Metainfo {
    fn announce(&self) -> Option<&str>;
    fn announce_list(&self) -> Option<&[Vec<String>]>;
    fn name(&self) -> &str;
    fn version(&self) -> u8;
    fn info_hash(&self) -> &[u8];
    fn piece_length(&self) -> u64;
    fn total_size(&self) -> u64;
    fn is_private(&self) -> bool;
    fn files(&self) -> &[EmbededFile];
    fn web_seeds(&self) -> Option<&[String]>;
    fn comment(&self) -> Option<&str>;
    fn created_by(&self) -> Option<&str>;
    fn creation_date(&self) -> Option<u64>;
    fn piece_hashes(&self) -> Vec<Vec<u8>>;
    fn raw_pieces(&self) -> &[u8];

    fn num_pieces(&self) -> usize {
        if self.piece_length() == 0 {
            return 0;
        }

        ((self.total_size() + (self.piece_length() as u64) - 1) / (self.piece_length() as u64))
            as usize
    }
}

#[enum_dispatch]
pub trait Integrity {
    fn validate(&self) -> Result<(), TorrentError>;
}

#[derive(Debug, Clone)]
pub enum InfoHash {
    V1(InfoHashV1),
    V2(InfoHashV2),
}

#[derive(Debug, Clone)]
pub struct InfoHashV1(Hash2OBytes);

impl InfoHashV1 {
    pub fn new(bytes: &[u8]) -> Self {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(&bytes);

        Self(hasher.finalize().into())
    }
}

#[derive(Debug, Clone)]
pub struct InfoHashV2(Hash32Bytes);

impl InfoHashV2 {
    pub fn new(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);

        Self(hasher.finalize().into())
    }
}

impl AsRef<[u8]> for InfoHash {
    fn as_ref(&self) -> &[u8] {
        match self {
            InfoHash::V1(h) => h.0.as_ref(),
            InfoHash::V2(h) => h.0.as_ref(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TorrentCommon {
    pub info_hash: InfoHash,
    pub name: String,
    pub announce: Option<String>,
    pub announce_list: Option<Vec<Vec<String>>>,
    pub piece_length: ByteSize,
    pub creation_date: Option<UnixDate>,
    pub comment: Option<String>,
    pub created_by: Option<String>,
    pub web_seeds: Option<Vec<String>>,
}

impl TorrentCommon {
    pub fn new(
        info_hash: InfoHash,
        name: String,
        announce: Option<String>,
        announce_list: Option<Vec<Vec<String>>>,
        piece_length: ByteSize,
        creation_date: Option<UnixDate>,
        comment: Option<String>,
        created_by: Option<String>,
        web_seeds: Option<Vec<String>>,
    ) -> Self {
        Self {
            info_hash,
            name,
            announce,
            announce_list,
            piece_length,
            creation_date,
            comment,
            created_by,
            web_seeds,
        }
    }
}
