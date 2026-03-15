use std::collections::HashMap;

use crate::{
    bit_torrent::torrent::{
        codec::{Metadata, MetadataFileTreeNode},
        metainfo::{InfoHash, Integrity, Metainfo, TorrentCommon, TorrentError, v1::EmbededFile},
    },
    types::{Hash32Bytes, MerkleRoot, PieceByte},
};

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

impl FileTreeNode {
    pub fn from_metadata(metadata: &MetadataFileTreeNode) -> Self {
        match metadata {
            MetadataFileTreeNode::File { entry } => FileTreeNode::File {
                entry: FileTreeEntry {
                    length: entry.length,
                    pieces_root: entry.pieces_root,
                },
            },
            MetadataFileTreeNode::Dir(files) => {
                let converted = files
                    .iter()
                    .map(|(name, child_node)| {
                        (name.clone(), FileTreeNode::from_metadata(child_node))
                    })
                    .collect();

                FileTreeNode::Dir(converted)
            }
        }
    }
}

pub struct TorrentV2 {
    pub(crate) info: TorrentCommon,
    pub(crate) version: Option<u8>,
    pub(crate) piece_layers: HashMap<MerkleRoot, PieceByte>,
    pub(crate) file_tree: Option<HashMap<String, FileTreeNode>>,
    pub(crate) flat_files: Vec<EmbededFile>,
    pub(crate) total_size: u64,
}

impl TorrentV2 {
    pub fn new(
        info: TorrentCommon,
        version: Option<u8>,
        piece_layers: HashMap<MerkleRoot, PieceByte>,
        file_tree: Option<HashMap<String, FileTreeNode>>,
        flat_files: Vec<EmbededFile>,
        total_size: u64,
    ) -> Self {
        Self {
            info,
            version,
            piece_layers,
            file_tree,
            flat_files,
            total_size,
        }
    }

    pub fn from_metadata(
        metadata: Metadata,
        info_hash: InfoHash,
    ) -> Result<TorrentV2, TorrentError> {
        let mut flat_files = Vec::new();
        let mut total_size = 0u64;
        let web_seeds = metadata.web_seeds();

        // FIX is missing transform this into domain's file tree
        if let Some(ref tree) = metadata.info.file_tree {
            for (name, node) in tree {
                let mut path = vec![name.clone()];
                total_size += Self::walk_and_flatten(node, &mut path, &mut flat_files);
            }
        }

        Ok(TorrentV2::new(
            TorrentCommon {
                info_hash,
                name: metadata.info.name,
                announce: metadata.announce,
                announce_list: metadata.announce_list,
                piece_length: metadata.info.piece_length,
                creation_date: metadata.creation_date,
                comment: metadata.comment,
                created_by: metadata.created_by,
                web_seeds: web_seeds,
            },
            metadata.info.version,
            metadata.piece_layers,
            None, // FIX it's none, must have value
            flat_files,
            total_size,
        ))
    }

    fn walk_and_flatten(
        node: &MetadataFileTreeNode,
        current_path: &mut Vec<String>,
        out: &mut Vec<EmbededFile>,
    ) -> u64 {
        match node {
            MetadataFileTreeNode::File { entry } => {
                let length = entry.length;
                out.push(EmbededFile {
                    length: length,
                    path: current_path.clone(),
                    md5sum: None,
                });

                length as u64
            }
            MetadataFileTreeNode::Dir(children) => {
                let mut dir_size = 0;
                for (name, child) in children {
                    current_path.push(name.clone());
                    dir_size += Self::walk_and_flatten(&child, current_path, out);
                    current_path.pop();
                }

                dir_size
            }
        }
    }
}

impl Integrity for TorrentV2 {
    fn validate(&self) -> Result<(), TorrentError> {
        todo!()
    }
}

impl Metainfo for TorrentV2 {
    fn announce(&self) -> Option<&str> {
        self.info.announce.as_deref()
    }

    fn announce_list(&self) -> Option<&[Vec<String>]> {
        self.info.announce_list.as_deref()
    }

    fn name(&self) -> &str {
        &self.info.name
    }

    fn version(&self) -> u8 {
        self.version.unwrap_or(2)
    }

    fn info_hash(&self) -> &[u8] {
        self.info.info_hash.as_ref()
    }

    fn piece_length(&self) -> u64 {
        self.info.piece_length as u64
    }

    fn total_size(&self) -> u64 {
        self.total_size
    }

    // FIX make it possible on torrent v2, to support mixed versions
    fn is_private(&self) -> bool {
        false
    }

    fn files(&self) -> &[EmbededFile] {
        &self.flat_files
    }

    fn web_seeds(&self) -> Option<&[String]> {
        self.info.web_seeds.as_deref()
    }

    fn comment(&self) -> Option<&str> {
        self.info.comment.as_deref()
    }

    fn created_by(&self) -> Option<&str> {
        self.info.created_by.as_deref()
    }

    fn creation_date(&self) -> Option<u64> {
        self.info.creation_date
    }

    fn piece_hashes(&self) -> Vec<Vec<u8>> {
        self.piece_layers
            .values()
            .map(|bytes| bytes.to_vec())
            .collect()
    }

    fn raw_pieces(&self) -> &[u8] {
        &[]
    }
}
