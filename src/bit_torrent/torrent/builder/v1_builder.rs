use std::{fs, path::PathBuf};

use crate::{
    bit_torrent::{
        bencode::{self, BencodeInfo, BencodeMode},
        torrent::{
            hasher::{PieceHasher, PieceHasherV1},
            metainfo::{
                InfoHash, InfoHashV1, TorrentCommon, TorrentError, TorrentFile, v1::TorrentV1,
            },
        },
        types::ByteSize,
    },
    types::{PieceByte, UnixDate},
    utils::bqti,
};

pub struct V1Builder {
    name: String,
    private: bool,
    piece_length: ByteSize,
    paths: Vec<PathBuf>,
    announce: Option<String>,
    announce_list: Option<Vec<Vec<String>>>,
    web_seeds: Option<Vec<String>>,
    creation_date: Option<u64>,
    comment: Option<String>,
    created_by: Option<String>,
}

#[allow(dead_code)]
impl V1Builder {
    pub(crate) fn new(name: impl Into<String>, piece_length: ByteSize) -> Self {
        Self {
            name: name.into(),
            piece_length,
            paths: Vec::new(),
            private: false,
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

    pub fn private(mut self, value: bool) -> Self {
        self.private = value;
        self
    }

    fn info_hash(&self, pieces: &PieceByte, mode: &BencodeMode) -> Result<InfoHash, TorrentError> {
        let private_flag = self.private.then_some(1u8);

        let meta_info = BencodeInfo::v1(
            self.name.clone(),
            private_flag,
            pieces.clone(),
            self.piece_length,
            mode.clone(),
        );

        match bencode::info_hash(&meta_info) {
            Ok(info_hash) => Ok(InfoHash::V1(InfoHashV1::new(&info_hash))),
            Err(e) => Err(TorrentError::Failed(e.to_string())),
        }
    }

    pub fn build(self) -> Result<TorrentFile, TorrentError> {
        // TODO check the piece length, must be to the power of two

        if self.paths.is_empty() {
            return Err(TorrentError::NotValid(
                "no files added to the builder".into(),
            ));
        }

        let mut hasher = PieceHasherV1::new(self.piece_length as usize);
        self.paths.iter().fold(&mut hasher, |h, p| h.file(p));

        let (pieces, mode) = hasher.finalize()?;
        let info_hash = self.info_hash(&pieces, &BencodeMode::from(mode.clone()))?;

        Ok(TorrentFile::V1(TorrentV1::new(
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
            self.private,
            pieces,
            mode,
        )))
    }
}
