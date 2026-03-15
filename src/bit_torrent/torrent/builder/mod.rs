use crate::{
    bit_torrent::torrent::{
        codec::{self, Metadata, MetadataMode},
        metainfo::{InfoHash, InfoHashV1, TorrentError, TorrentFile, v1::TorrentV1},
    },
    types::{ByteSize, PieceByte},
};

mod v1_builder;
mod v2_builder;

pub use v1_builder::V1Builder;
pub use v2_builder::V2Builder;

#[derive(Debug)]
pub struct TorrentBuilder {}

impl TorrentBuilder {
    pub fn with_v2() -> V2Builder {
        V2Builder {}
    }

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
        let raw_hash =
            codec::info_hash(&metadata.info).map_err(|e| TorrentError::Failed(e.to_string()))?;

        match (has_pieces, has_file_tree) {
            (true, false) => {
                let info_hash = InfoHash::V1(InfoHashV1::new(&raw_hash));
                let torrent = TorrentV1::from_metadata(metadata, info_hash)?;

                Ok(TorrentFile::V1(torrent))
            }
            (false, true) => Err(TorrentError::Failed("not implemented yet".into())),
            (true, true) => Err(TorrentError::Unsupported(
                "Hybrid v1+v2 not yet implemented".into(),
            )),
            (false, false) => Err(TorrentError::Unsupported("no matching version".into())),
        }
    }
}
