mod v1_builder;

pub use v1_builder::V1Builder;

use crate::{
    bit_torrent::torrent::{
        codec::{Metadata, MetadataMode},
        metainfo::{TorrentError, TorrentFile},
    },
    types::{ByteSize, PieceByte},
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

    pub fn apply_from_metadata(metadata: Metadata) -> Result<TorrentFile, TorrentError> {
        let has_pieces = !metadata.info.pieces.is_empty();
        let has_file_tree = metadata.info.file_tree.is_some();

        match (has_pieces, has_file_tree) {
            (true, false) => TorrentBuilder::build_v1_from_metadata(metadata),
            (false, true) => Err(TorrentError::Unsupported("BitTorrent v2 only".into())),
            (true, true) => Err(TorrentError::Unsupported(
                "Hybrid v1+v2 not yet implemented".into(),
            )),
            (false, false) => Err(TorrentError::Unsupported("no matching version".into())),
        }
    }

    fn build_v1_from_metadata(metadata: Metadata) -> Result<TorrentFile, TorrentError> {
        let is_private = metadata.info.is_private();
        let web_seeds = metadata.web_seeds();

        let Some(mode) = metadata.info.mode() else {
            return Err(TorrentError::UnsupportedVersion(1));
        };

        let mut builder = TorrentBuilder::with_v1(
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
