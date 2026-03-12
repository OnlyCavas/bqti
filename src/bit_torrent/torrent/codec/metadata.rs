use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_bencode::value::Value;

use crate::bit_torrent::{
    torrent::{
        codec::{MetadataInfo, info::FileInfo},
        metainfo::Metainfo,
    },
    types::{MerkleRoot, PieceByte, UnixDate},
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

    pub fn from_metainfo(metainfo: &impl Metainfo) -> Metadata {
        Metadata {
            announce: metainfo.announce().map(|s| s.to_string()),
            info: Metadata::from_info(metainfo),
            announce_list: metainfo.announce_list().map(|l| l.to_vec()),
            creation_date: metainfo.creation_date(),
            comment: metainfo.comment().map(|s| s.to_string()),
            created_by: metainfo.created_by().map(|s| s.to_string()),
            url_list: metainfo.web_seeds().map(|l| l.to_vec()),
            extra: HashMap::new(),
            piece_layers: HashMap::new(),
        }
    }

    fn from_info(metainfo: &impl Metainfo) -> MetadataInfo {
        let all_files = metainfo.files();

        let (length, files) = if all_files.len() == 1 {
            (Some(all_files[0].length as i64), None)
        } else {
            let converted = all_files
                .into_iter()
                .map(FileInfo::from)
                .collect::<Vec<FileInfo>>();

            (None, Some(converted))
        };
        MetadataInfo {
            name: metainfo.name().to_string(),
            version: Some(metainfo.version()),
            piece_length: metainfo.piece_length() as i64,
            private: if metainfo.is_private() {
                Some(1)
            } else {
                Some(0)
            },
            length,
            files,
            file_tree: None,
            pieces: serde_bytes::ByteBuf::from(metainfo.raw_pieces().to_vec()),
            md5sum: None,
        }
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
