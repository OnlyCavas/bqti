use std::{path::PathBuf, str::FromStr};

use crate::{
    bit_torrent::{
        bencode::{self, BencodeInfo, BencodeMode},
        torrent::{
            metainfo::{
                InfoHash, InfoHashV1, PieceLength, TorrentCommon, TorrentError, TorrentFile,
                v1::TorrentV1,
            },
            path::TorrentPath,
            piece_hash::{PieceHasher, PieceHasherV1},
        },
        types::ByteSize,
    },
    torrent::metainfo::TorrentAddr,
    types::{PieceByte, UnixDate},
    utils::bqti,
};

pub struct V1Builder {
    name: String,
    private: bool,
    piece_length: PieceLength,
    files: TorrentPath,
    announce: Option<String>,
    announce_list: Option<Vec<Vec<String>>>,
    web_seeds: Option<Vec<String>>,
    creation_date: Option<u64>,
    comment: Option<String>,
    created_by: Option<String>,
    dht_nodes: Option<Vec<TorrentAddr>>,
}

#[allow(dead_code)]
impl V1Builder {
    pub(crate) fn new(
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        piece_length: ByteSize,
    ) -> Self {
        Self {
            name: name.into(),
            piece_length: PieceLength::from(piece_length),
            files: TorrentPath::new(path),
            private: false,
            announce: None,
            announce_list: None,
            web_seeds: None,
            creation_date: None,
            comment: None,
            created_by: None,
            dht_nodes: None,
        }
    }

    pub fn file(mut self, input: impl Into<PathBuf>) -> Self {
        let input_path = input.into();
        self.files = self.files.add(input_path);

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

    pub fn dht_nodes(mut self, nodes: impl Into<Option<Vec<String>>>) -> Self {
        self.dht_nodes = nodes.into().map(|vec| {
            vec.into_iter()
                .filter_map(|s| TorrentAddr::from_str(&s).ok())
                .collect()
        });

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
            self.piece_length.0 as i64,
            mode.clone(),
        );

        match bencode::info_hash(&meta_info) {
            Ok(info_hash) => Ok(InfoHash::V1(InfoHashV1::new(&info_hash))),
            Err(e) => Err(TorrentError::Failed(e.to_string())),
        }
    }

    pub fn build(self) -> Result<TorrentFile, TorrentError> {
        self.piece_length.validate()?;

        if self.files.is_empty() {
            return Err(TorrentError::NotValid(
                "the provided path contains no files".into(),
            ));
        }

        let files = self.files.clone();
        let mut hasher = PieceHasherV1::new(self.piece_length.0 as usize);

        for (abs, rel) in &files.build() {
            hasher.file(abs, rel);
        }

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
                self.dht_nodes,
            ),
            self.private,
            pieces,
            mode,
        )))
    }
}
