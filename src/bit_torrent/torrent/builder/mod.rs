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

        let mode = metadata
            .info
            .mode()
            .ok_or(TorrentError::UnsupportedVersion(1))?;

        TorrentBuilder::with_v1(
            metadata.info.name,
            metadata.info.piece_length,
            metadata.info.pieces,
            mode,
        )
        .announce(metadata.announce)
        .announce_list(metadata.announce_list)
        .web_seeds(web_seeds)
        .creation_date(metadata.creation_date)
        .comment(metadata.comment)
        .created_by(metadata.created_by)
        .private(is_private)
        .build()
    }
}
