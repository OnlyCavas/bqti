use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_bencode::value::Value;

use crate::{
    torrent::metainfo::TorrentAddr,
    types::{ByteSize, MerkleRoot, PieceByte, UnixDate},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BencodeTorrent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub announce: Option<String>, // sigle or main tracker both v1 and v2

    pub info: BencodeInfo, // torrent info, both v1 and v2

    #[serde(
        rename = "announce-list",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub announce_list: Option<Vec<Vec<String>>>, // fallback tracker list, v1 optional

    #[serde(
        rename = "creation date",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub creation_date: Option<UnixDate>, // created at, v1 but compatible

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>, // comment, v1 but compatible

    #[serde(
        rename = "created by",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub created_by: Option<String>, // created by, v1 but compatible

    #[serde(rename = "url-list", skip_serializing_if = "Option::is_none")]
    url_list: Option<Vec<String>>, // BEP 19 (GetRight), fallback and compatible

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    extra: HashMap<String, serde_bencode::value::Value>,

    #[serde(
        rename = "piece layers",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub piece_layers: HashMap<MerkleRoot, PieceByte>, // only on v2, n * 32

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nodes: Option<Vec<(String, u16)>>,
}

impl BencodeTorrent {
    pub fn new(
        announce: Option<String>,
        info: BencodeInfo,
        announce_list: Option<Vec<Vec<String>>>,
        creation_date: Option<UnixDate>,
        comment: Option<String>,
        created_by: Option<String>,
        url_list: Option<Vec<String>>,
        extra: HashMap<String, serde_bencode::value::Value>,
        piece_layers: HashMap<MerkleRoot, PieceByte>,
        nodes: Option<Vec<(String, u16)>>,
    ) -> Self {
        Self {
            announce,
            info,
            announce_list,
            creation_date,
            comment,
            created_by,
            url_list,
            extra,
            piece_layers,
            nodes,
        }
    }

    fn extra_value(&self, id: &str) -> Option<&serde_bencode::value::Value> {
        self.extra.get(id)
    }

    pub fn dht_nodes(&self) -> Option<Vec<TorrentAddr>> {
        self.nodes.as_ref().map(|nodes| {
            nodes
                .iter()
                .filter_map(|(ip_str, port)| Some(TorrentAddr::from_raw_parts(ip_str, *port).ok()?))
                .collect()
        })
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

#[derive(Debug, Clone)]
pub enum BencodeMode {
    SingleFile {
        length: ByteSize,
        md5sum: Option<String>,
    },
    MultiFile {
        files: Vec<FileInfo>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BencodeInfo {
    pub name: String, // file name

    #[serde(
        rename = "meta version",
        default,
        skip_serializing_if = "Option::is_none"
    )] // some versions
    pub(crate) version: Option<u8>, // only v2

    #[serde(rename = "file tree", default, skip_serializing_if = "Option::is_none")]
    pub file_tree: Option<HashMap<String, BencodeFileTreeNode>>,

    #[serde(rename = "private", default, skip_serializing_if = "Option::is_none")] // some versions
    pub(crate) private: Option<u8>, //only v1 must not use PEX or DHT

    pub pieces: PieceByte, // chunk old version

    #[serde(rename = "piece length")]
    pub piece_length: ByteSize, // each piece length

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) length: Option<ByteSize>, // single file

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) md5sum: Option<String>, // single file

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) files: Option<Vec<FileInfo>>, // multiple embeded files
}

impl BencodeInfo {
    pub fn v1(
        name: String,
        private: Option<u8>,
        pieces: PieceByte,
        piece_length: ByteSize,
        mode: BencodeMode,
    ) -> Self {
        let (length, md5sum, files) = match mode {
            BencodeMode::SingleFile { length, md5sum } => (Some(length), md5sum, None),
            BencodeMode::MultiFile { files } => (None, None, Some(files)),
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

    pub fn mode(&self) -> Option<BencodeMode> {
        match (&self.length, &self.md5sum, &self.files) {
            (Some(length), Some(md5sum), None) => Some(BencodeMode::SingleFile {
                length: *length,
                md5sum: Some(md5sum.clone()),
            }),
            (Some(length), None, None) => Some(BencodeMode::SingleFile {
                length: *length,
                md5sum: None,
            }),
            (None, None, Some(files)) => Some(BencodeMode::MultiFile {
                files: files.clone(),
            }),
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BencodeFileTreeEntry {
    pub length: i64,
    #[serde(rename = "pieces root", default)]
    pub pieces_root: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BencodeFileTreeNode {
    File {
        #[serde(rename = "")]
        entry: BencodeFileTreeEntry,
    },
    Dir(HashMap<String, BencodeFileTreeNode>),
}
