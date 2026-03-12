use std::collections::HashMap;

use thiserror::Error;

use crate::bit_torrent::types::{ByteSize, Hash2OBytes, Hash32Bytes, MerkleRoot, PieceByte};

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

pub enum TorrentMode<'a> {
    SingleFile {
        length: ByteSize,
        md5sum: &'a Option<String>,
    },
    MultiFile {
        files: &'a Vec<EmbededFile>,
    },
}

#[derive(Debug, strum::Display)]
pub enum TorrentFile {
    V1(TorrentV1),
    V2(TorrentV2),
}

impl TorrentFile {
    fn common(&self) -> &TorrentCommon {
        match self {
            TorrentFile::V1(torrent_v1) => &torrent_v1.info,
            TorrentFile::V2(torrent_v2) => &torrent_v2.info,
        }
    }

    pub fn name(&self) -> String {
        self.common().name.clone()
    }

    pub fn version(&self) -> String {
        self.to_string()
    }

    pub fn announce(&self) -> Option<String> {
        self.common().announce.clone()
    }

    pub fn announce_list(&self) -> Option<Vec<Vec<String>>> {
        self.common().announce_list.clone()
    }

    pub fn info_hash(&self) -> &[u8] {
        self.common().info_hash.as_ref()
    }

    pub fn web_seeds(&self) -> Option<Vec<String>> {
        self.common().web_seeds.clone()
    }

    pub fn piece_length(&self) -> ByteSize {
        self.common().piece_length
    }

    pub fn creation_date(&self) -> Option<ByteSize> {
        self.common().creation_date
    }

    pub fn comment(&self) -> Option<String> {
        self.common().comment.clone()
    }

    pub fn created_by(&self) -> Option<String> {
        self.common().created_by.clone()
    }

    pub fn hashes(&self) -> Result<Vec<Hash2OBytes>, TorrentError> {
        match self {
            TorrentFile::V1(v1) => v1.hashes(),
            TorrentFile::V2(_) => todo!(),
        }
    }

    pub fn mode(&self) -> TorrentMode<'_> {
        match self {
            TorrentFile::V1(v1) => match &v1.mode {
                V1Mode::SingleFile { length, md5sum } => TorrentMode::SingleFile {
                    length: *length,
                    md5sum,
                },
                V1Mode::MultiFile { files } => TorrentMode::MultiFile { files },
            },
            TorrentFile::V2(_) => todo!(),
        }
    }

    pub fn total_size(&self) -> Option<ByteSize> {
        match self.mode() {
            TorrentMode::SingleFile { length, md5sum: _ } => Some(length),
            TorrentMode::MultiFile { files } => Some(files.iter().map(|f| f.length).sum()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum InfoHash {
    V1(Hash2OBytes),
    V2(Hash32Bytes),
}

impl AsRef<[u8]> for InfoHash {
    fn as_ref(&self) -> &[u8] {
        match self {
            InfoHash::V1(h) => h.as_ref(),
            InfoHash::V2(h) => h.as_ref(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TorrentCommon {
    pub(crate) info_hash: InfoHash,
    pub(crate) name: String,
    pub(crate) announce: Option<String>,
    pub(crate) announce_list: Option<Vec<Vec<String>>>,
    pub(crate) web_seeds: Option<Vec<String>>,
    pub(crate) piece_length: ByteSize,          // each piece length
    pub(crate) creation_date: Option<ByteSize>, // created at, v1 but compatible
    pub(crate) comment: Option<String>,         // comment, v1 but compatible
    pub(crate) created_by: Option<String>,      // created by, v1 but compatible
}

// Metafile Torrent V1

#[derive(Debug)]
pub struct TorrentV1 {
    pub(crate) info: TorrentCommon,
    pub private: bool,     //only v1 must not use PEX or DHT convert to bool
    pub pieces: PieceByte, // chunk old version
    pub mode: V1Mode,
}

impl TorrentV1 {
    pub fn new(
        info_hash: Hash2OBytes,
        name: String,
        announce: Option<String>,
        announce_list: Option<Vec<Vec<String>>>,
        web_seeds: Option<Vec<String>>,
        piece_length: ByteSize,
        private: bool,
        pieces: PieceByte,
        mode: V1Mode,
        creation_date: Option<ByteSize>,
        comment: Option<String>,
        created_by: Option<String>,
    ) -> Self {
        Self {
            info: TorrentCommon {
                info_hash: InfoHash::V1(info_hash),
                name,
                announce,
                announce_list,
                web_seeds,
                piece_length,
                creation_date,
                comment,
                created_by,
            },
            private,
            pieces,
            mode,
        }
    }

    pub fn hashes(&self) -> Result<Vec<Hash2OBytes>, TorrentError> {
        self.pieces
            .chunks_exact(20)
            .map(|c| c.try_into().map_err(|_| TorrentError::Hash20Error()))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub enum V1Mode {
    SingleFile {
        length: ByteSize,
        md5sum: Option<String>,
    },
    MultiFile {
        files: Vec<EmbededFile>,
    },
}

#[derive(Debug, Clone)]
pub struct EmbededFile {
    pub length: ByteSize,       // size of each file
    pub path: Vec<String>,      // path
    pub md5sum: Option<String>, // file md5 sum
}

// Metafile Torrent V2
#[derive(Debug)]
pub struct TorrentV2 {
    info: TorrentCommon,
    pub piece_layers: HashMap<MerkleRoot, PieceByte>,
    version: Option<u8>,
    file_tree: Option<HashMap<String, FileTreeNode>>,
    pub mode: V1Mode,
}

impl TorrentV2 {
    pub fn new(
        info_hash: Hash32Bytes,
        name: String,
        announce: Option<String>,
        announce_list: Option<Vec<Vec<String>>>,
        web_seeds: Option<Vec<String>>,
        piece_length: ByteSize,
        creation_date: Option<ByteSize>,
        comment: Option<String>,
        created_by: Option<String>,
        piece_layers: HashMap<MerkleRoot, PieceByte>,
        version: Option<u8>,
        file_tree: Option<HashMap<String, FileTreeNode>>,
        mode: V1Mode,
    ) -> Self {
        Self {
            info: TorrentCommon {
                info_hash: InfoHash::V2(info_hash),
                name,
                announce,
                announce_list,
                web_seeds,
                piece_length,
                creation_date,
                comment,
                created_by,
            },
            piece_layers,
            version,
            file_tree,
            mode,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileTreeEntry {
    pub length: i64,
    pub pieces_root: Option<Hash32Bytes>,
}

#[derive(Debug, Clone)]
pub enum FileTreeNode {
    File { entry: FileTreeEntry },
    Dir(HashMap<String, FileTreeNode>),
}
