use std::{collections::HashMap, fs, path::PathBuf};

use crate::{
    bit_torrent::{
        bencode::{self, BencodeInfo, BencodeMode},
        torrent::{
            metainfo::{
                InfoHash, InfoHashV2, PieceLength, TorrentCommon, TorrentError, TorrentFile,
                v1::EmbededFile,
                v2::{FileTreeNode, TorrentV2},
            },
            piece_hash::{PieceHasher, PieceHasherV1},
        },
    },
    types::{ByteSize, PieceByte, UnixDate},
    utils::bqti,
};

pub struct V2Builder {
    name: String,
    paths: Vec<PathBuf>,
    piece_length: PieceLength,
    announce: Option<String>,
    announce_list: Option<Vec<Vec<String>>>,
    web_seeds: Option<Vec<String>>,
    creation_date: Option<u64>,
    comment: Option<String>,
    created_by: Option<String>,
    piece_layers: Option<Vec<EmbededFile>>,
    file_tree: Option<HashMap<String, FileTreeNode>>,
}

impl V2Builder {
    pub fn new(name: impl Into<String>, piece_length: ByteSize) -> Self {
        Self {
            name: name.into(),
            piece_length: PieceLength::from(piece_length),
            paths: Vec::new(),
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

    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();

        if !path.is_dir() {
            self.paths.push(path);
            return self;
        }

        if let Ok(files) = fs::read_dir(&path) {
            for entry in files.flatten() {
                self = self.file(entry.path());
            }
        }

        self
    }

    pub fn files(mut self, file_paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        for path in file_paths {
            self = self.file(path);
        }

        self
    }

    pub fn announce(mut self, url: impl Into<String>) -> Self {
        let url: String = url.into();

        let tiers = self.announce_list.get_or_insert_with(|| vec![Vec::new()]);

        if tiers.is_empty() {
            tiers.push(Vec::new());
        }

        tiers[0].push(url.clone());

        if self.announce.is_none() {
            self.announce = Some(url);
        }

        self
    }

    pub fn announce_list(mut self, tiers: impl Into<Vec<Vec<String>>>) -> Self {
        self.announce_list = Some(tiers.into());

        self.announce = self
            .announce_list
            .as_ref()
            .and_then(|l| l.first())
            .and_then(|t| t.first())
            .cloned();

        self
    }

    pub fn web_seeds(mut self, urls: impl Into<Option<Vec<String>>>) -> Self {
        self.web_seeds = urls.into();
        self
    }

    pub fn comment(mut self, comment: impl Into<Option<String>>) -> Self {
        self.comment = comment.into();
        self
    }

    pub fn created_by(mut self, created_by: impl Into<Option<String>>) -> Self {
        self.created_by = created_by.into();
        self
    }

    pub fn creation_date(mut self, date: impl Into<Option<UnixDate>>) -> Self {
        self.creation_date = date.into();
        self
    }

    fn info_hash(&self, pieces: &PieceByte, mode: &BencodeMode) -> Result<InfoHash, TorrentError> {
        let meta_info = BencodeInfo::v1(
            self.name.clone(),
            None,
            pieces.clone(),
            self.piece_length.0 as i64,
            mode.clone(),
        );

        match bencode::info_hash(&meta_info) {
            Ok(info_hash) => Ok(InfoHash::V2(InfoHashV2::new(&info_hash))),
            Err(e) => Err(TorrentError::Failed(e.to_string())),
        }
    }

    pub fn build(self) -> Result<TorrentFile, TorrentError> {
        self.piece_length.validate()?;

        if self.paths.is_empty() {
            return Err(TorrentError::NotValid(
                "no files added to the builder".into(),
            ));
        }

        let mut hasher = PieceHasherV1::new(self.piece_length.0 as usize);
        self.paths.iter().fold(&mut hasher, |h, p| h.file(p));

        let (pieces, mode) = hasher.finalize()?;
        let info_hash = self.info_hash(&pieces, &BencodeMode::from(mode.clone()))?;

        Ok(TorrentFile::V2(TorrentV2::new(
            TorrentCommon::new(
                info_hash,
                self.name,
                self.announce,
                self.announce_list,
                self.piece_length,
                self.creation_date.or(Some(bqti::fetch_current_timestamp())),
                self.comment,
                self.created_by.or(Some(bqti::version())),
                self.web_seeds,
            ),
            Some(2),        // version
            HashMap::new(), //piece_layers,
            None,           // file tree
            None,           // flatten
            0,              // total size
        )))
    }
}
