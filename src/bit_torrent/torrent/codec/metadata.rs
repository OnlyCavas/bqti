use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_bencode::value::Value;

use crate::{
    bit_torrent::{
        torrent::codec::MetadataInfo,
        types::{MerkleRoot, PieceByte, UnixDate},
    },
    torrent::{
        codec::info::FileInfo,
        torrent::{TorrentFile, V1Mode},
    },
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Metadata {
    pub announce: Option<String>, // sigle or main tracker both v1 and v2
    pub info: MetadataInfo,       // torrent info, both v1 and v2

    #[serde(rename = "announce-list", default)]
    pub announce_list: Option<Vec<Vec<String>>>, // fallback tracker list, v1 optional

    #[serde(rename = "creation date", default)]
    pub creation_date: Option<UnixDate>, // created at, v1 but compatible

    #[serde(default)]
    pub comment: Option<String>, // comment, v1 but compatible

    #[serde(rename = "created by", default)]
    pub created_by: Option<String>, // created by, v1 but compatible

    #[serde(rename = "url-list")]
    url_list: Option<Vec<String>>, // BEP 19 (GetRight), fallback and compatible

    #[serde(flatten)]
    extra: HashMap<String, serde_bencode::value::Value>,

    #[serde(rename = "piece layers", default)]
    pub piece_layers: HashMap<MerkleRoot, PieceByte>, // only on v2, n * 32
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

fn from_info(value: &TorrentFile) -> MetadataInfo {
    let version = value.version().parse::<u8>().unwrap_or(0);

    match value {
        TorrentFile::V1(v1) => {
            let (length, md5sum, files) = match &v1.mode {
                V1Mode::SingleFile { length, md5sum } => (Some(*length), md5sum.clone(), None),
                V1Mode::MultiFile { files } => {
                    let entries = files
                        .iter()
                        .map(|f| FileInfo {
                            length: f.length,
                            path: f.path.clone(),
                            md5sum: f.md5sum.clone(),
                        })
                        .collect();

                    (None, None, Some(entries))
                }
            };

            MetadataInfo {
                name: value.name(),
                version: Some(version),
                file_tree: None,
                private: v1.private.then_some(1u8),
                pieces: v1.pieces.clone(),
                piece_length: value.piece_length(),
                length,
                md5sum,
                files,
            }
        }
        TorrentFile::V2(_v2) => todo!(),
    }
}

impl From<&TorrentFile> for Metadata {
    fn from(value: &TorrentFile) -> Self {
        Metadata {
            announce: value.announce(),
            info: from_info(value),
            announce_list: value.announce_list(),
            creation_date: value.creation_date(),
            comment: value.comment(),
            created_by: value.created_by(),
            url_list: value.web_seeds(),
            extra: HashMap::new(),
            piece_layers: HashMap::new(), // v2
        }
    }
}
