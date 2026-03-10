use crate::bit_torrent::torrent::{
    reader::{Metadata, MetadataMode},
    torrent::{EmbededFile, TorrentError, TorrentFile, TorrentV1, V1Mode},
};

#[derive(Debug)]
pub struct TorrentBuilder {}

impl TorrentBuilder {
    pub fn from_metadata(metadata: Metadata) -> Result<TorrentFile, TorrentError> {
        let Some(mode) = metadata.info.mode().map(|e| match e {
            MetadataMode::SingleFile { length, md5sum } => V1Mode::SingleFile { length, md5sum },
            MetadataMode::MultiFile { files } => V1Mode::MultiFile {
                files: files.into_iter().map(EmbededFile::from).collect(),
            },
        }) else {
            return Err(TorrentError::UnsupportedVersion(1));
        };

        Ok(TorrentFile::V1(TorrentV1::new(
            [0u8; 20],
            metadata.info.name.clone(),
            metadata.announce.clone(),
            metadata.announce_list.clone(),
            metadata.web_seeds(),
            metadata.info.piece_length,
            metadata.info.is_private(),
            metadata.info.pieces.clone(),
            mode,
            metadata.creation_date,
            metadata.comment,
            metadata.created_by,
        )))
    }

    // fn build() {}
}
