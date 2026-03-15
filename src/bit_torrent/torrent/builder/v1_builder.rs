use crate::{
    bit_torrent::{
        torrent::{
            codec::{self, MetadataInfo, MetadataMode},
            metainfo::{
                InfoHash, InfoHashV1, TorrentCommon, TorrentError, TorrentFile,
                v1::{TorrentV1, V1Mode},
            },
        },
        types::{ByteSize, PieceByte},
    },
    types::UnixDate,
};

pub struct V1Builder {
    name: String,
    private: bool,
    piece_length: ByteSize,
    pieces: PieceByte,
    mode: MetadataMode,
    announce: Option<String>,
    announce_list: Option<Vec<Vec<String>>>,
    web_seeds: Option<Vec<String>>,
    creation_date: Option<u64>,
    comment: Option<String>,
    created_by: Option<String>,
}

impl V1Builder {
    pub(crate) fn new(
        name: String,
        piece_length: ByteSize,
        pieces: PieceByte,
        mode: MetadataMode,
    ) -> Self {
        Self {
            name,
            piece_length,
            pieces,
            private: false,
            announce: None,
            announce_list: None,
            web_seeds: None,
            creation_date: None,
            comment: None,
            created_by: None,
            mode,
        }
    }

    pub fn announce(mut self, url: impl Into<Option<String>>) -> Self {
        if let Some(url) = url.into() {
            let tiers = self.announce_list.get_or_insert_with(|| vec![Vec::new()]);

            if tiers.is_empty() {
                tiers.push(Vec::new());
            }

            tiers[0].push(url.clone());

            if self.announce.is_none() {
                self.announce = Some(url);
            }
        }
        self
    }

    pub fn announce_list(mut self, tiers: impl Into<Option<Vec<Vec<String>>>>) -> Self {
        if let Some(ts) = tiers.into() {
            self.announce_list = Some(ts);

            self.announce = self
                .announce_list
                .as_ref()
                .and_then(|l| l.first())
                .and_then(|t| t.first())
                .cloned();
        }
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

    fn info_hash(&self) -> Result<InfoHash, TorrentError> {
        let private_flag = if self.private { Some(1u8) } else { None };

        let meta_info = MetadataInfo::v1(
            self.name.clone(),
            private_flag,
            self.pieces.clone(),
            self.piece_length,
            self.mode.clone(),
        );

        match codec::info_hash(&meta_info) {
            Ok(info_hash) => Ok(InfoHash::V1(InfoHashV1::new(&info_hash))),
            Err(e) => Err(TorrentError::Failed(e.to_string())),
        }
    }

    pub fn build(self) -> Result<TorrentFile, TorrentError> {
        let file_path = self.name.clone();
        let info_hash = self.info_hash()?;
        let mode = V1Mode::from_metadata_mode(self.mode, file_path);

        Ok(TorrentFile::V1(TorrentV1::new(
            TorrentCommon::new(
                info_hash,
                self.name,
                self.announce,
                self.announce_list,
                self.piece_length,
                self.creation_date,
                self.comment,
                self.created_by,
                self.web_seeds,
            ),
            self.private,
            self.pieces,
            mode,
        )))
    }
}
