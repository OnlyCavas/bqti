use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_bencode::value::Value;

use crate::bit_torrent::{ByteSize, MerkleRoot, PieceByte, torrent::torrent::EmbededFile};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Metadata {
    pub announce: Option<String>, // sigle or main tracker both v1 and v2

    // 0052
    pub info: MetadataInfo, // torrent info, both v1 and v2

    #[serde(rename = "announce-list", default)]
    pub announce_list: Option<Vec<Vec<String>>>, // fallback tracker list, v1 optional

    #[serde(rename = "creation date", default)]
    pub creation_date: Option<ByteSize>, // created at, v1 but compatible

    #[serde(default)]
    pub comment: Option<String>, // comment, v1 but compatible

    #[serde(rename = "created by", default)]
    pub created_by: Option<String>, // created by, v1 but compatible

    #[serde(rename = "url-list")]
    url_list: Option<Vec<String>>, // BEP 19 (GetRight), fallback and compatible

    #[serde(flatten)]
    extra: HashMap<String, serde_bencode::value::Value>,

    // the key is the merkle root and value are n * 32
    #[serde(rename = "piece layers", default)]
    pub piece_layers: HashMap<MerkleRoot, PieceByte>, // only on v2
}

impl Metadata {
    fn extra_value(&self, id: &str) -> Option<&serde_bencode::value::Value> {
        self.extra.get(id)
    }

    pub fn web_seeds(&self) -> Option<Vec<String>> {
        let Some(mut seeds) = self.url_list.clone() else {
            return None;
        };

        if seeds.is_empty() {
            return None;
        }

        let Some(Value::List(httpseed)) = self.extra_value("httpseeds") else {
            return Some(seeds);
        };

        for seed in httpseed {
            let Value::Bytes(item) = seed else {
                continue;
            };

            if let Ok(url) = String::from_utf8(item.clone()) {
                seeds.push(url);
            }
        }

        Some(seeds)
    }
}

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
    version: Option<u8>, // only v2

    #[serde(rename = "file tree", default)]
    file_tree: Option<HashMap<String, FileTreeNode>>,

    #[serde(rename = "private", default)] // some versions
    private: Option<u8>, //only v1 must not use PEX or DHT

    pub pieces: PieceByte, // chunk old version

    #[serde(rename = "piece length")]
    pub piece_length: ByteSize, // each piece length

    length: Option<ByteSize>,     // single file
    md5sum: Option<String>,       // single file
    files: Option<Vec<FileInfo>>, // multiple embeded files
}

impl MetadataInfo {
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
pub struct FileTreeEntry {
    pub length: i64,
    #[serde(rename = "pieces root", default)]
    pub pieces_root: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FileTreeNode {
    File {
        #[serde(rename = "")]
        entry: FileTreeEntry,
    },
    Dir(HashMap<String, FileTreeNode>),
}
