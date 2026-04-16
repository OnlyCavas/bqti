use std::{collections::HashMap, net::SocketAddr};

use clap::error::Result;

use crate::{
    bit_torrent::{
        bencode::{BencodeFileTreeNode, BencodeTorrent},
        magnet::MagnetLink,
        torrent::{
            merkle::MerkleTree,
            metainfo::{
                InfoHash, Integrity, Metainfo, PieceIntegrity, PieceLength, TorrentCommon,
                TorrentError, v1::EmbededFile,
            },
        },
    },
    hasher::Sha256Hash,
    torrent::metainfo::Magnet,
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

impl From<&BencodeFileTreeNode> for FileTreeNode {
    fn from(value: &BencodeFileTreeNode) -> Self {
        match value {
            BencodeFileTreeNode::File { entry } => FileTreeNode::File {
                entry: FileTreeEntry {
                    length: entry.length,
                    pieces_root: entry.pieces_root,
                },
            },
            BencodeFileTreeNode::Dir(files) => {
                let converted = files
                    .iter()
                    .map(|(name, child_node)| (name.clone(), FileTreeNode::from(child_node)))
                    .collect();

                FileTreeNode::Dir(converted)
            }
        }
    }
}

#[derive(Clone)]
pub struct TorrentV2 {
    pub(crate) info: TorrentCommon,
    pub(crate) version: Option<u8>,
    pub(crate) piece_layers: HashMap<MerkleRoot, PieceByte>,
    file_tree: Option<HashMap<String, FileTreeNode>>,
    flat_files: Option<Vec<EmbededFile>>,
    total_size: u64,
}

impl TorrentV2 {
    pub fn new(
        info: TorrentCommon,
        version: Option<u8>,
        piece_layers: HashMap<MerkleRoot, PieceByte>,
        file_tree: Option<HashMap<String, FileTreeNode>>,
        flat_files: Option<Vec<EmbededFile>>,
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

    pub fn from_bencode(
        metadata: BencodeTorrent,
        info_hash: InfoHash,
    ) -> Result<TorrentV2, TorrentError> {
        let mut flat_files = Vec::new();
        let mut total_size = 0;

        let web_seeds = metadata.web_seeds();
        let dht_nodes = metadata.dht_nodes();

        let file_tree = metadata.info.file_tree.as_ref().map(|tree| {
            tree.iter()
                .map(|(name, node)| (name.clone(), FileTreeNode::from(node)))
                .collect::<HashMap<String, FileTreeNode>>()
        });

        if let Some(ref tree) = file_tree {
            let mut path_buffer = Vec::new();
            for (name, node) in tree {
                total_size += Self::walk_and_flatten(name, node, &mut path_buffer, &mut flat_files);
            }
        }

        Ok(TorrentV2::new(
            TorrentCommon {
                info_hash,
                name: metadata.info.name,
                announce: metadata.announce,
                announce_list: metadata.announce_list,
                piece_length: PieceLength::from(metadata.info.piece_length),
                creation_date: metadata.creation_date,
                comment: metadata.comment,
                created_by: metadata.created_by,
                web_seeds,
                dht_nodes,
            },
            metadata.info.version,
            metadata.piece_layers,
            file_tree,
            Some(flat_files),
            total_size,
        ))
    }

    fn walk_and_flatten(
        name: &str,
        node: &FileTreeNode,
        current_path: &mut Vec<String>,
        out: &mut Vec<EmbededFile>,
    ) -> u64 {
        current_path.push(name.to_string());

        let size = match node {
            FileTreeNode::File { entry } => {
                out.push(EmbededFile {
                    length: entry.length,
                    path: current_path.clone(),
                    md5sum: None,
                });

                entry.length as u64
            }
            FileTreeNode::Dir(children) => {
                let mut dir_size = 0;

                for (child_name, child_node) in children {
                    dir_size += Self::walk_and_flatten(child_name, child_node, current_path, out);
                }

                dir_size
            }
        };

        current_path.pop();
        size
    }

    fn check_integrity(&self, node: &FileTreeNode) -> Result<(), TorrentError> {
        match node {
            FileTreeNode::File { entry } => {
                // if the file is smaller then the piece length
                if entry.length <= self.piece_length().0 as i64 {
                    return Ok(());
                }

                let Some(expected_root) = entry.pieces_root else {
                    return Err(TorrentError::NotValid("missing piece root".into()));
                };

                let layers = self
                    .piece_layers
                    .get(&expected_root)
                    .ok_or_else(|| TorrentError::NotValid("missing layer root".into()))?;

                let merkle_tree = MerkleTree::from_piece_layers(&layers)?;

                if merkle_tree.root != expected_root {
                    return Err(TorrentError::NotValid(
                        format!(
                            "mismatch roots! Expected: {}, got {}",
                            hex::encode(merkle_tree.root),
                            hex::encode(expected_root)
                        )
                        .into(),
                    ));
                }
            }
            FileTreeNode::Dir(files) => {
                for file in files.values() {
                    self.check_integrity(&file)?;
                }
            }
        }

        Ok(())
    }
}

impl Magnet for TorrentV2 {
    fn magnet(&self) -> MagnetLink {
        MagnetLink {
            hash: self.info_hash().to_string(),
            name: Some(self.name().to_string()),
            bootstrap: self.dht_nodes().map(|nodes| {
                nodes
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            }),
            trackers: self
                .announce_list()
                .map(|tiers| tiers.iter().flatten().cloned().collect())
                .unwrap_or_default(),
            web_seed: self.web_seeds().and_then(|seeds| seeds.first().cloned()),
        }
    }
}

impl PieceIntegrity for TorrentV2 {
    fn verify_hash(&self, index: u32, data: &[u8]) -> Result<(), TorrentError> {
        let hashes = self.piece_hashes();

        let expected = hashes
            .get(index as usize)
            .ok_or(TorrentError::NotValid(format!(
                "no hash for piece {}",
                index
            )))?;

        let actual = Sha256Hash::digest(data);

        if actual.as_bytes() != expected.as_slice() {
            return Err(TorrentError::NotValid(format!(
                "piece {} hash mismatch: expected {:?}, got {:?}",
                index,
                expected,
                actual.as_bytes()
            )));
        }

        Ok(())
    }
}

impl Integrity for TorrentV2 {
    fn validate(&self) -> Result<(), TorrentError> {
        let Some(file_tree) = self.file_tree.as_ref() else {
            return Err(TorrentError::NotValid("file tree did not compose".into()));
        };

        for node in file_tree.values() {
            self.check_integrity(node)?;
        }

        Ok(())
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

    fn info_hash(&self) -> &InfoHash {
        &self.info.info_hash
    }

    fn piece_length(&self) -> PieceLength {
        self.info.piece_length
    }

    fn total_size(&self) -> u64 {
        self.total_size
    }

    fn is_private(&self) -> bool {
        false
    }

    fn files(&self) -> &[EmbededFile] {
        self.flat_files.as_deref().unwrap_or(&[])
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

    fn dht_nodes(&self) -> Option<&[SocketAddr]> {
        self.info.dht_nodes.as_deref()
    }
}
