use std::collections::HashMap;

use crate::{
    bit_torrent::torrent::metainfo::{v1::EmbededFile, v2::FileTreeNode},
    types::ByteSize,
};

pub struct V2Builder {
    name: String,
    piece_length: ByteSize,
    piece_layers: Option<Vec<EmbededFile>>,
    file_tree: Option<HashMap<String, FileTreeNode>>,
    announce: Option<String>,
    announce_list: Option<Vec<Vec<String>>>,
    web_seeds: Option<Vec<String>>,
    creation_date: Option<u64>,
    comment: Option<String>,
    created_by: Option<String>,
}

impl V2Builder {
    pub fn new(name: String, piece_length: ByteSize) -> Self {
        Self {
            name,
            piece_length,
            piece_layers: None,
            file_tree: None,
            announce: None,
            announce_list: None,
            web_seeds: None,
            creation_date: None,
            comment: None,
            created_by: None,
        }
    }
}
