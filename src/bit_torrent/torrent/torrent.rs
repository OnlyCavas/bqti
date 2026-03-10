use thiserror::Error;

use crate::bit_torrent::{ByteSize, Hash2OBytes, PieceByte};

#[derive(Error, Debug)]
pub enum TorrentError {
    #[error("piece hash doesn't have 20 bytes")]
    Hash20Error(),

    #[error("unsupported version, {0}")]
    UnsupportedVersion(u8),
}

// Metafile Torrent V1

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
}

impl TorrentFile {
    pub fn name(&self) -> String {
        match self {
            TorrentFile::V1(v1) => v1.name.clone(),
        }
    }

    pub fn version(&self) -> String {
        self.to_string()
    }

    pub fn announce(&self) -> Option<String> {
        match self {
            TorrentFile::V1(v1) => v1.announce.clone(),
        }
    }

    pub fn announce_list(&self) -> Option<Vec<Vec<String>>> {
        match self {
            TorrentFile::V1(v1) => v1.announce_list.clone(),
        }
    }

    pub fn info_hash(&self) -> &[u8] {
        match self {
            TorrentFile::V1(v1) => v1.info_hash.as_ref(),
        }
    }

    pub fn web_seeds(&self) -> Option<Vec<String>> {
        match self {
            TorrentFile::V1(v1) => v1.web_seeds.clone(),
        }
    }

    pub fn piece_length(&self) -> ByteSize {
        match self {
            TorrentFile::V1(v1) => v1.piece_length,
        }
    }

    pub fn creation_date(&self) -> Option<ByteSize> {
        match self {
            TorrentFile::V1(v1) => v1.creation_date,
        }
    }

    pub fn comment(&self) -> Option<String> {
        match self {
            TorrentFile::V1(v1) => v1.comment.clone(),
        }
    }

    pub fn created_by(&self) -> Option<String> {
        match self {
            TorrentFile::V1(v1) => v1.created_by.clone(),
        }
    }

    pub fn piece_hashes(&self) -> Result<Vec<Hash2OBytes>, TorrentError> {
        match self {
            TorrentFile::V1(v1) => v1
                .pieces
                .chunks(20)
                .map(|c| -> Result<Hash2OBytes, TorrentError> {
                    c.try_into().map_err(|_| TorrentError::Hash20Error())
                })
                .collect(),
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
        }
    }

    pub fn total_size(&self) -> Option<ByteSize> {
        match self.mode() {
            TorrentMode::SingleFile { length, md5sum: _ } => Some(length),
            TorrentMode::MultiFile { files } => Some(files.iter().map(|f| f.length).sum()),
        }
    }
}

#[derive(Debug)]
pub struct TorrentV1 {
    info_hash: Hash2OBytes,
    name: String,
    announce: Option<String>,
    announce_list: Option<Vec<Vec<String>>>,
    web_seeds: Option<Vec<String>>,
    piece_length: ByteSize,          // each piece length
    creation_date: Option<ByteSize>, // created at, v1 but compatible
    comment: Option<String>,         // comment, v1 but compatible
    created_by: Option<String>,      // created by, v1 but compatible

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
            info_hash,
            name,
            announce,
            announce_list,
            web_seeds,
            piece_length,
            private,
            pieces,
            mode,
            creation_date,
            comment,
            created_by,
        }
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

//
// impl Torrent {
//
//

// }

// #[derive(Debug)]
// pub enum TorrentMode {
//     FileTree {
//         tree: HashMap<String, FileTreeNode>,
//     },
//     SingleFile {
//         length: ByteSize,
//         md5sum: Option<String>,
//     },
//     MultiFile {
//         files: Vec<FileInfo>, // v1
//
//     },
// }
//
// #[derive(Debug)]
// pub enum TorrentVersion {
//     V1,
//     V2,
//     Hybrid,
// }

// impl Info {
//     pub fn piece_length(&self) -> ByteSize {
//         self.piece_length
//     }
//
//     pub fn version(&self) -> Result<TorrentVersion, TorrentError> {
//         match (self.version, self.is_hybrid()) {
//             (None, _) if self.file_tree.is_none() => Ok(TorrentVersion::V1),
//             (Some(2), true) => Ok(TorrentVersion::Hybrid),
//             (Some(2), false) => Ok(TorrentVersion::V2),
//             (Some(1), _) => Err(TorrentError::UnsupportedVersion(1)),
//             (Some(v), _) => Err(TorrentError::UnsupportedVersion(v)),
//             _ => Ok(TorrentVersion::V1),
//         }
//     }
//     pub fn is_hybrid(&self) -> bool {
//         !self.pieces.is_empty() && self.file_tree.is_some()
//     }
//
//     // FIX keep atention that File tree may coexist with multi file or single file
//    //

// }
//
