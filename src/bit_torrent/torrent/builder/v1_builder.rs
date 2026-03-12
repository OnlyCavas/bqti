use sha1::{Digest, Sha1};

use crate::bit_torrent::{
    torrent::{
        codec::{self, MetadataInfo, MetadataMode},
        torrent::{EmbededFile, TorrentError, TorrentFile, TorrentV1, V1Mode},
    },
    types::{ByteSize, Hash2OBytes, PieceByte},
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
    creation_date: Option<ByteSize>,
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

    pub fn announce(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        let tiers = self.announce_list.get_or_insert_with(Vec::new);

        if tiers.is_empty() {
            tiers.push(vec![url.clone()]);
            self.announce = Some(url);
        } else {
            tiers[0].push(url);
            self.announce = tiers[0].first().cloned();
        }

        self
    }

    pub fn announce_list(mut self, tiers: Vec<Vec<String>>) -> Self {
        self.announce_list = Some(tiers);

        self.announce = self
            .announce_list
            .as_ref()
            .and_then(|ts| ts.first())
            .and_then(|tier| tier.first())
            .cloned();

        self
    }

    pub fn web_seeds(mut self, urls: Vec<String>) -> Self {
        self.web_seeds = Some(urls);
        self
    }

    pub fn private(mut self, private: bool) -> Self {
        self.private = private;
        self
    }

    pub fn creation_date(mut self, unix_timestamp: i64) -> Self {
        self.creation_date = Some(unix_timestamp);
        self
    }

    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }

    pub fn created_by(mut self, created_by: impl Into<String>) -> Self {
        self.created_by = Some(created_by.into());
        self
    }

    fn info_hash(&self) -> Result<Hash2OBytes, TorrentError> {
        let private_flag = if self.private { Some(1u8) } else { None };

        // FIX remove clone
        let meta_info = MetadataInfo::v1(
            self.name.clone(),
            private_flag,
            self.pieces.clone(),
            self.piece_length,
            self.mode.clone(),
        );

        match codec::info_hash(&meta_info) {
            Ok(info_hash) => Ok(Sha1::digest(info_hash).into()),
            Err(e) => Err(TorrentError::Failed(e.to_string())),
        }
    }

    pub fn build(self) -> Result<TorrentFile, TorrentError> {
        let info_hash = self.info_hash()?;

        let mode = match self.mode {
            MetadataMode::SingleFile { length, md5sum } => V1Mode::SingleFile { length, md5sum },
            MetadataMode::MultiFile { files } => V1Mode::MultiFile {
                files: files.into_iter().map(EmbededFile::from).collect(),
            },
        };

        Ok(TorrentFile::V1(TorrentV1::new(
            info_hash,
            self.name,
            self.announce,
            self.announce_list,
            self.web_seeds,
            self.piece_length,
            self.private,
            self.pieces,
            mode,
            self.creation_date,
            self.comment,
            self.created_by,
        )))
    }
}
