use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::bit_torrent::{
    torrent::torrent::EmbededFile,
    types::{ByteSize, PieceByte},
};

#[derive(Debug, Clone)]
pub enum MetadataMode {
    SingleFile {
        length: ByteSize,
        md5sum: Option<String>,
    },
    MultiFile {
        files: Vec<FileInfo>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetadataInfo {
    pub name: String, // file name

    #[serde(rename = "meta version", default)] // some versions
    pub(crate) version: Option<u8>, // only v2

    #[serde(rename = "file tree", default)]
    pub file_tree: Option<HashMap<String, MetadataFileTreeEntry>>,

    #[serde(rename = "private", default)] // some versions
    pub(crate) private: Option<u8>, //only v1 must not use PEX or DHT

    pub pieces: PieceByte, // chunk old version

    #[serde(rename = "piece length")]
    pub piece_length: ByteSize, // each piece length

    pub(crate) length: Option<ByteSize>,     // single file
    pub(crate) md5sum: Option<String>,       // single file
    pub(crate) files: Option<Vec<FileInfo>>, // multiple embeded files
}

impl MetadataInfo {
    pub fn v1(
        name: String,
        private: Option<u8>,
        pieces: PieceByte,
        piece_length: ByteSize,
        mode: MetadataMode,
    ) -> Self {
        let (length, md5sum, files) = match mode {
            MetadataMode::SingleFile { length, md5sum } => (Some(length), md5sum, None),
            MetadataMode::MultiFile { files } => (None, None, Some(files)),
        };

        Self {
            name,
            version: None,
            file_tree: None,
            private,
            pieces,
            piece_length,
            length,
            md5sum,
            files,
        }
    }

    pub fn is_private(&self) -> bool {
        self.private.is_some_and(|v| v != 0)
    }

    pub fn mode(&self) -> Option<MetadataMode> {
        match (&self.length, &self.md5sum, &self.files) {
            (Some(length), Some(md5sum), None) => Some(MetadataMode::SingleFile {
                length: *length,
                md5sum: Some(md5sum.clone()),
            }),
            (Some(length), None, None) => Some(MetadataMode::SingleFile {
                length: *length,
                md5sum: None,
            }),
            (None, None, Some(files)) => Some(MetadataMode::MultiFile {
                files: files.clone(),
            }),
            // (None, None, None, Some(file_tree)) => Some(TorrentMode::FileTree {
            //     tree: file_tree.clone(),
            // }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileInfo {
    pub length: ByteSize,       // size of each file
    pub path: Vec<String>,      // path
    pub md5sum: Option<String>, // file md5 sum
}

impl From<FileInfo> for EmbededFile {
    fn from(f: FileInfo) -> Self {
        EmbededFile {
            length: f.length,
            path: f.path,
            md5sum: f.md5sum,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetadataFileTreeEntry {
    pub length: i64,
    #[serde(rename = "pieces root", default)]
    pub pieces_root: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MetadataFileTreeNode {
    File {
        #[serde(rename = "")]
        entry: MetadataFileTreeEntry,
    },
    Dir(HashMap<String, MetadataFileTreeNode>),
}
