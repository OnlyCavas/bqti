use std::path::PathBuf;

use crate::{
    bit_torrent::{
        bencode::{self, BencodeTorrent},
        torrent::metainfo::{
            InfoHash, InfoHashV1, InfoHashV2, TorrentError, TorrentFile, v1::TorrentV1,
            v2::TorrentV2,
        },
    },
    types::ByteSize,
};

mod v1_builder;
mod v2_builder;

pub use v1_builder::V1Builder;
pub use v2_builder::V2Builder;

#[derive(Debug)]
pub struct TorrentBuilder {}

impl TorrentBuilder {
    pub fn with_v2(
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        piece_length: ByteSize,
    ) -> V2Builder {
        V2Builder::new(name, path, piece_length)
    }

    pub fn with_v1(
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        piece_length: ByteSize,
    ) -> V1Builder {
        V1Builder::new(name, path, piece_length)
    }

    pub fn apply_from_metadata(metadata: BencodeTorrent) -> Result<TorrentFile, TorrentError> {
        let has_pieces = !metadata.info.pieces.is_empty();
        let has_file_tree = metadata.info.file_tree.is_some();
        let raw_hash =
            bencode::info_hash(&metadata.info).map_err(|e| TorrentError::Failed(e.to_string()))?;

        match (has_pieces, has_file_tree) {
            (true, false) => {
                let info_hash = InfoHash::V1(InfoHashV1::new(&raw_hash));
                let torrent = TorrentV1::from_bencode(metadata, info_hash)?;

                Ok(TorrentFile::V1(torrent))
            }
            (false, true) => {
                let info_hash = InfoHash::V2(InfoHashV2::new(&raw_hash));
                let torrent = TorrentV2::from_bencode(metadata, info_hash)?;

                Ok(TorrentFile::V2(torrent))
            }
            (true, true) => Err(TorrentError::Unsupported(
                "Hybrid v1+v2 not yet implemented".into(),
            )),
            (false, false) => Err(TorrentError::Unsupported("no matching version".into())),
        }
    }
}
