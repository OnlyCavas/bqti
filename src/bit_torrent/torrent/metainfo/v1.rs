use std::{net::SocketAddr, path::PathBuf};

use crate::{
    bit_torrent::{
        bencode::{BencodeMode, BencodeTorrent, FileInfo},
        magnet::MagnetLink,
        torrent::metainfo::{
            InfoHash, Integrity, Metainfo, PieceIntegrity, PieceLength, TorrentCommon, TorrentError,
        },
    },
    hasher::Sha1Hash,
    torrent::metainfo::Magnet,
    types::{ByteSize, PieceByte},
};

#[derive(Debug, Clone)]
pub enum V1Mode {
    SingleFile { file: EmbededFile },
    MultiFile { files: Vec<EmbededFile> },
}

impl V1Mode {
    pub fn from_bencode_mode(mode: &BencodeMode, torrent_name: String) -> Self {
        match mode {
            BencodeMode::SingleFile { length, md5sum } => V1Mode::SingleFile {
                file: EmbededFile {
                    length: *length,
                    path: vec![torrent_name],
                    md5sum: md5sum.clone(),
                },
            },
            BencodeMode::MultiFile { files } => V1Mode::MultiFile {
                files: files.iter().map(|f| EmbededFile::from(f.clone())).collect(),
            },
        }
    }
}

impl From<V1Mode> for BencodeMode {
    fn from(value: V1Mode) -> Self {
        match value {
            V1Mode::SingleFile { file } => BencodeMode::SingleFile {
                length: file.length,
                md5sum: file.md5sum,
            },
            V1Mode::MultiFile { files } => BencodeMode::MultiFile {
                files: files.iter().map(|f| FileInfo::from(f.clone())).collect(),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmbededFile {
    pub length: ByteSize,
    pub path: Vec<String>,
    pub md5sum: Option<String>,
}

impl EmbededFile {
    pub fn to_path(&self) -> PathBuf {
        self.path.iter().collect()
    }
}

impl From<FileInfo> for EmbededFile {
    fn from(value: FileInfo) -> Self {
        EmbededFile {
            length: value.length,
            path: value.path,
            md5sum: value.md5sum,
        }
    }
}

impl From<EmbededFile> for FileInfo {
    fn from(value: EmbededFile) -> Self {
        FileInfo {
            length: value.length,
            path: value.path,
            md5sum: value.md5sum,
        }
    }
}

#[derive(Clone)]
pub struct TorrentV1 {
    pub(crate) info: TorrentCommon,
    pub(crate) private: bool,
    pub(crate) pieces: PieceByte,
    pub(crate) mode: V1Mode,
}

impl TorrentV1 {
    pub fn new(info: TorrentCommon, private: bool, pieces: PieceByte, mode: V1Mode) -> Self {
        Self {
            info,
            private,
            pieces,
            mode,
        }
    }

    pub fn from_bencode(
        metadata: BencodeTorrent,
        info_hash: InfoHash,
    ) -> Result<TorrentV1, TorrentError> {
        let web_seeds = metadata.web_seeds();
        let dht_nodes = metadata.dht_nodes();
        let file_path = metadata.info.name.clone();
        let private = metadata.info.private.unwrap_or(0) == 1;

        let mode = metadata
            .info
            .mode()
            .map(|mode| V1Mode::from_bencode_mode(&mode, file_path))
            .ok_or(TorrentError::NotValid(
                "Unable to determine file mode".into(),
            ))?;

        Ok(TorrentV1::new(
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
            private,
            metadata.info.pieces,
            mode,
        ))
    }
}

impl Magnet for TorrentV1 {
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

impl PieceIntegrity for TorrentV1 {
    fn verify_hash(&self, index: u32, data: &[u8]) -> Result<(), TorrentError> {
        let hashes = self.piece_hashes();

        let expected = hashes
            .get(index as usize)
            .ok_or(TorrentError::NotValid(format!(
                "no hash for piece {}",
                index
            )))?;

        let actual = Sha1Hash::digest(data);

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

impl Integrity for TorrentV1 {
    fn validate(&self) -> Result<(), TorrentError> {
        let total_size = self.total_size();
        let pl_len = self.piece_length();

        pl_len.validate()?;
        let expected_pieces = (total_size + pl_len.0 - 1) / pl_len.0;

        if self.pieces.len() % 20 != 0 {
            return Err(TorrentError::NotValid("the pieces exceed 20 bytes".into()));
        }

        let actual_hashes = (self.pieces.len() / 20) as u64;
        if expected_pieces != actual_hashes {
            return Err(TorrentError::NotValid(format!(
                "mismatch: expecting {} hashes, found {}",
                expected_pieces, actual_hashes
            )));
        }

        if total_size == 0 {
            return Err(TorrentError::NotValid("empty torrent".into()));
        }

        Ok(())
    }
}

impl Metainfo for TorrentV1 {
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
        1
    }

    fn info_hash(&self) -> &InfoHash {
        &self.info.info_hash
    }

    fn piece_length(&self) -> PieceLength {
        self.info.piece_length
    }

    fn total_size(&self) -> u64 {
        match &self.mode {
            V1Mode::SingleFile { file } => file.length as u64,
            V1Mode::MultiFile { files } => files.iter().map(|file| file.length as u64).sum(),
        }
    }

    fn is_private(&self) -> bool {
        self.private
    }

    fn files(&self) -> &[EmbededFile] {
        match &self.mode {
            V1Mode::SingleFile { file } => std::slice::from_ref(file),
            V1Mode::MultiFile { files } => files.as_slice(),
        }
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
        self.pieces
            .chunks_exact(20)
            .map(|chunk| chunk.to_vec())
            .collect()
    }

    fn raw_pieces(&self) -> &[u8] {
        &self.pieces
    }

    fn dht_nodes(&self) -> Option<&[SocketAddr]> {
        self.info.dht_nodes.as_deref()
    }
}
