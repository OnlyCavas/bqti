use sha1::{Digest, Sha1};

use crate::bit_torrent::{
    ByteSize, Hash2OBytes, PieceByte,
    torrent::{
        reader::{Metadata, MetadataInfo, MetadataMode},
        torrent::{EmbededFile, TorrentError, TorrentFile, TorrentV1, V1Mode},
    },
};

#[derive(Debug)]
pub struct TorrentBuilder {}

impl TorrentBuilder {
    pub fn with_v1(
        name: String,
        piece_length: ByteSize,
        pieces: PieceByte,
        mode: MetadataMode,
    ) -> V1Builder {
        V1Builder::new(name, piece_length, pieces, mode)
    }

    pub fn from_metadata(metadata: Metadata) -> Result<TorrentFile, TorrentError> {
        let has_pieces = !metadata.info.pieces.is_empty();
        let has_file_tree = metadata.info.file_tree.is_some();

        match (has_pieces, has_file_tree) {
            (true, false) => TorrentBuilder::create_v1(metadata),
            (false, true) => Err(TorrentError::Unsupported("BitTorrent v2 only".into())),
            (true, true) => Err(TorrentError::Unsupported(
                "Hybrid v1+v2 not yet implemented".into(),
            )),
            (false, false) => Err(TorrentError::Unsupported("no matching version".into())),
        }
    }

    fn create_v1(metadata: Metadata) -> Result<TorrentFile, TorrentError> {
        let is_private = metadata.info.is_private();
        let web_seeds = metadata.web_seeds();

        let Some(mode) = metadata.info.mode() else {
            return Err(TorrentError::UnsupportedVersion(0));
        };

        let mut builder = V1Builder::new(
            metadata.info.name,
            metadata.info.piece_length,
            metadata.info.pieces,
            mode,
        );

        if let Some(url) = metadata.announce {
            builder = builder.announce(url);
        }

        if let Some(tiers) = metadata.announce_list {
            builder = builder.announce_list(tiers);
        }

        if let Some(seeds) = web_seeds {
            builder = builder.web_seeds(seeds);
        }

        if let Some(ts) = metadata.creation_date {
            builder = builder.creation_date(ts);
        }

        if let Some(c) = metadata.comment {
            builder = builder.comment(c);
        }

        if let Some(by) = metadata.created_by {
            builder = builder.created_by(by);
        }

        builder = builder.private(is_private);
        builder.build()
    }
}

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
    fn new(name: String, piece_length: ByteSize, pieces: PieceByte, mode: MetadataMode) -> Self {
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

        if self.announce.is_none() {
            self.announce = Some(url);
            return self;
        }

        self.announce_list
            .get_or_insert_with(Vec::new)
            .push(vec![url]);

        self
    }

    pub fn announce_list(mut self, tiers: Vec<Vec<String>>) -> Self {
        self.announce_list = Some(tiers);

        let Some(first_tier) = self.announce_list.as_ref().and_then(|v| v.first()) else {
            return self;
        };

        let Some(first_url) = first_tier.first() else {
            return self;
        };

        self.announce = Some(first_url.clone());
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

    fn calculate_info_hash(&self) -> Result<Hash2OBytes, TorrentError> {
        let info_header = &MetadataInfo::v1(
            self.name.clone(),
            None,
            self.pieces.clone(),
            self.piece_length,
            self.mode.clone(),
        );

        let info_bytes =
            serde_bencode::to_bytes(&info_header).map_err(|_| TorrentError::Hash20Error())?;

        Ok(Sha1::digest(info_bytes).into())
    }

    pub fn build(self) -> Result<TorrentFile, TorrentError> {
        let info_hash = self.calculate_info_hash()?;

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
