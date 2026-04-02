use std::{collections::HashMap, net::SocketAddr};

use enum_dispatch::enum_dispatch;
use thiserror::Error;

use crate::{
    bit_torrent::{
        bencode::{BencodeInfo, BencodeTorrent, FileInfo},
        torrent::metainfo::{
            v1::{EmbededFile, TorrentV1},
            v2::TorrentV2,
        },
    },
    hasher::{Sha1Hash, Sha256Hash},
    types::{Hash2OBytes, Hash32Bytes, UnixDate},
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

#[enum_dispatch(Metainfo, Integrity, PieceIntegrity)]
#[derive(Clone)]
pub enum TorrentFile {
    V1(TorrentV1),
    V2(TorrentV2),
}

#[enum_dispatch]
pub trait Metainfo {
    fn announce(&self) -> Option<&str>;
    fn announce_list(&self) -> Option<&[Vec<String>]>;
    fn dht_nodes(&self) -> Option<&[SocketAddr]>;
    fn name(&self) -> &str;
    fn version(&self) -> u8;
    fn info_hash(&self) -> &InfoHash;
    fn piece_length(&self) -> PieceLength;
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
        if self.piece_length() == PieceLength(0) {
            return 0;
        }

        ((self.total_size() + (self.piece_length().0) - 1) / (self.piece_length().0)) as usize
    }
}

#[enum_dispatch]
pub trait Integrity {
    fn validate(&self) -> Result<(), TorrentError>;
}

#[enum_dispatch]
pub trait PieceIntegrity {
    fn verify_hash(&self, index: u32, data: &[u8]) -> Result<(), TorrentError>;
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum InfoHash {
    V1(InfoHashV1),
    V2(InfoHashV2),
}

impl InfoHash {
    pub fn to_string(&self) -> String {
        hex::encode(self.as_ref())
    }
}

#[derive(Debug, Hash, Clone, PartialEq, Eq)]
pub struct InfoHashV1(Hash2OBytes);

impl InfoHashV1 {
    pub fn new(bytes: &[u8]) -> Self {
        Self(*Sha1Hash::digest(bytes).as_bytes())
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct InfoHashV2(Hash32Bytes);

impl InfoHashV2 {
    pub fn new(bytes: &[u8]) -> Self {
        Self(*Sha256Hash::digest(bytes).as_bytes())
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

impl From<&InfoHash> for Vec<u8> {
    fn from(value: &InfoHash) -> Self {
        value.as_ref().to_vec()
    }
}

impl TryFrom<Vec<u8>> for InfoHash {
    type Error = String;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        match value.len() {
            20 => {
                let mut hash = [0u8; 20];
                hash.copy_from_slice(&value);

                Ok(InfoHash::V1(InfoHashV1(hash)))
            }
            32 => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&value);

                Ok(InfoHash::V2(InfoHashV2(hash)))
            }
            _ => Err(format!(
                "Invalid InfoHash length: {} bytes (expected 20 or 32)",
                value.len()
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TorrentCommon {
    pub info_hash: InfoHash,
    pub name: String,
    pub announce: Option<String>,
    pub announce_list: Option<Vec<Vec<String>>>,
    pub piece_length: PieceLength,
    pub creation_date: Option<UnixDate>,
    pub comment: Option<String>,
    pub created_by: Option<String>,
    pub web_seeds: Option<Vec<String>>,
    pub dht_nodes: Option<Vec<SocketAddr>>,
}

impl TorrentCommon {
    pub fn new(
        info_hash: InfoHash,
        name: String,
        announce: Option<String>,
        announce_list: Option<Vec<Vec<String>>>,
        piece_length: PieceLength,
        creation_date: Option<UnixDate>,
        comment: Option<String>,
        created_by: Option<String>,
        web_seeds: Option<Vec<String>>,
        dht_nodes: Option<Vec<SocketAddr>>,
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
            dht_nodes,
        }
    }
}

impl From<&TorrentFile> for BencodeInfo {
    fn from(value: &TorrentFile) -> Self {
        let all_files = value.files();

        let (length, files) = if all_files.len() == 1 {
            (Some(all_files[0].length as i64), None)
        } else {
            let converted: Vec<FileInfo> = all_files
                .iter()
                .map(|f| FileInfo::from(f.clone()))
                .collect();

            (None, Some(converted))
        };

        BencodeInfo {
            name: value.name().to_string(),
            version: (value.version() != 1).then_some(value.version()),
            piece_length: value.piece_length().0 as i64,
            private: value.is_private().then_some(1),
            length,
            files,
            file_tree: None,
            pieces: serde_bytes::ByteBuf::from(value.raw_pieces().to_vec()),
            md5sum: None,
        }
    }
}

impl From<&TorrentFile> for BencodeTorrent {
    fn from(value: &TorrentFile) -> Self {
        let dht_nodes: Option<Vec<(String, u16)>> = value.dht_nodes().map(|nodes_slice| {
            nodes_slice
                .iter()
                .map(|addr| (addr.ip().to_string(), addr.port()))
                .collect()
        });

        BencodeTorrent::new(
            value.announce().map(|s| s.to_string()),
            value.into(),
            value.announce_list().map(|l| l.to_vec()),
            value.creation_date(),
            value.comment().map(|s| s.to_string()),
            value.created_by().map(|s| s.to_string()),
            value.web_seeds().map(|l| l.to_vec()),
            HashMap::new(), // FIX missing the v2 impl
            HashMap::new(), // FIX missing the v2 impl
            dht_nodes,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PieceLength(pub u64);

impl PieceLength {
    pub fn value(&self) -> u64 {
        self.0
    }

    pub fn validate(self) -> Result<(), TorrentError> {
        let pl = self.0;
        const MIN_LIMIT: u64 = 16 * 1024; // 16 kb
        const MAX_LIMIT: u64 = 128 * 1024 * 1024; // 128 mb

        if pl < MIN_LIMIT || pl > MAX_LIMIT {
            return Err(TorrentError::NotValid(format!(
                "invalid piece length {}",
                pl
            )));
        }

        if (pl & (pl - 1)) != 0 {
            return Err(TorrentError::NotValid(
                "piece of length must be to the power of 2".into(),
            ));
        }

        Ok(())
    }
}

impl From<i64> for PieceLength {
    fn from(value: i64) -> Self {
        PieceLength(value as u64)
    }
}

impl PartialEq<u64> for PieceLength {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}
