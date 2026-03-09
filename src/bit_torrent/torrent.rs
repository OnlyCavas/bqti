use std::collections::HashMap;
use thiserror::Error;

use serde::{Deserialize, Serialize};
use serde_bencode::value::Value;

use crate::bit_torrent::{ByteSize, Hash2OBytes, PieceByte};

#[derive(Error, Debug)]
pub enum TorrentError {
    #[error("piece hash doesn't have 20 bytes")]
    Hash20Error(),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Torrent {
    pub info: Info, // torrent info

    pub announce: Option<String>, // sigle or main tracker

    #[serde(rename = "announce-list", default)]
    pub announce_list: Option<Vec<Vec<String>>>, // fallback tracker list

    #[serde(rename = "url-list")]
    url_list: Option<Vec<String>>, // BEP 19 (GetRight), fallback

    #[serde(default)]
    pub comment: Option<String>, // comment

    #[serde(rename = "created by", default)]
    pub created_by: Option<String>, // created by

    #[serde(rename = "creation date", default)]
    pub creation_date: Option<ByteSize>, // created at

    #[serde(flatten)]
    extra: HashMap<String, serde_bencode::value::Value>,
    // pub nodes: Option<Vec<(String, ByteSize)>>,
    #[serde(rename = "root hash", default)]
    pub pieces_root: Option<[u8; 20]>,
}

impl Torrent {
    pub fn new(
        info: Info,
        announce: Option<String>,
        announce_list: Option<Vec<Vec<String>>>,
        url_list: Option<Vec<String>>,
        comment: Option<String>,
        created_by: Option<String>,
        creation_date: Option<ByteSize>,
        extra: HashMap<String, serde_bencode::value::Value>,
        pieces_root: Option<[u8; 20]>,
    ) -> Self {
        Self {
            info,
            announce,
            announce_list,
            url_list,
            comment,
            created_by,
            creation_date,
            extra,
            pieces_root,
        }
    }

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

#[derive(Debug)]
pub enum TorrentMode {
    SingleFile { length: ByteSize },
    MultiFile { files: Vec<FileInfo> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Info {
    pub name: String, // file name

    pieces: PieceByte, // chunk

    #[serde(rename = "piece length")]
    piece_length: ByteSize, // each piece length

    length: Option<ByteSize>,     // single file
    files: Option<Vec<FileInfo>>, // multiple embeded files
}

impl Info {
    pub fn piece_length(&self) -> ByteSize {
        self.piece_length
    }

    pub fn piece_hashes(&self) -> Result<Vec<Hash2OBytes>, TorrentError> {
        self.pieces
            .chunks(20)
            .map(|c| -> Result<Hash2OBytes, TorrentError> {
                c.try_into().map_err(|_| TorrentError::Hash20Error())
            })
            .collect()
    }

    pub fn mode(&self) -> Option<TorrentMode> {
        match (&self.length, &self.files) {
            (Some(length), None) => Some(TorrentMode::SingleFile { length: *length }),
            (None, Some(files)) => Some(TorrentMode::MultiFile {
                files: files.clone(),
            }),
            _ => None,
        }
    }

    pub fn total_size(&self) -> Option<ByteSize> {
        match self.mode()? {
            TorrentMode::SingleFile { length } => Some(length),
            TorrentMode::MultiFile { files } => Some(files.iter().map(|f| f.length).sum()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileInfo {
    pub length: ByteSize,  // size of each file
    pub path: Vec<String>, // path
}
