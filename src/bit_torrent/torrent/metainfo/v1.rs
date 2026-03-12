use crate::{
    bit_torrent::torrent::metainfo::{
        EmbededFile, Metainfo, TorrentCommon, TorrentError, TorrentIntegrity, V1Mode,
    },
    types::PieceByte,
};

pub struct TorrentV1 {
    pub(crate) info: TorrentCommon,
    pub(crate) private: bool,
    pub(crate) pieces: PieceByte,
    pub(crate) mode: V1Mode,
}

impl TorrentV1 {
    pub fn new(info: TorrentCommon, private: bool, pieces: PieceByte, mode: V1Mode) -> Self {
        Self {
            info,
            private,
            pieces,
            mode,
        }
    }
}

impl TorrentIntegrity for TorrentV1 {
    fn validate(&self) -> Result<(), TorrentError> {
        let total_size = self.total_size();
        let pl_len = self.piece_length();

        const MIN_LIMIT: u64 = 16 * 1024; // 16 kb
        const MAX_LIMIT: u64 = 128 * 1024 * 1024; // 128 mb

        if pl_len < MIN_LIMIT || pl_len > MAX_LIMIT {
            return Err(TorrentError::NotValid(format!(
                "invalid piece length {}",
                pl_len
            )));
        }

        if (pl_len & (pl_len - 1)) != 0 {
            return Err(TorrentError::NotValid(
                "piece of length must be to the power of 2".into(),
            ));
        }

        let expected_pieces = (total_size + pl_len - 1) / pl_len;

        if self.pieces.len() % 20 != 0 {
            return Err(TorrentError::NotValid("the pieces exceed 20 bytes".into()));
        }

        let actual_hashes = (self.pieces.len() / 20) as u64;
        if expected_pieces != actual_hashes {
            return Err(TorrentError::NotValid(format!(
                "mismatch: expecting {} hashes, found {}",
                expected_pieces, actual_hashes
            )));
        }

        if total_size == 0 {
            return Err(TorrentError::NotValid("empty torrent".into()));
        }

        Ok(())
    }
}

impl Metainfo for TorrentV1 {
    fn announce(&self) -> Option<&str> {
        self.info.announce.as_deref()
    }

    fn announce_list(&self) -> Option<&[Vec<String>]> {
        self.info.announce_list.as_deref()
    }

    fn name(&self) -> &str {
        &self.info.name
    }

    fn version(&self) -> u8 {
        1
    }

    fn info_hash(&self) -> &[u8] {
        self.info.info_hash.as_ref()
    }

    fn piece_length(&self) -> u64 {
        self.info.piece_length as u64
    }

    fn total_size(&self) -> u64 {
        match &self.mode {
            V1Mode::SingleFile { length, .. } => *length as u64,
            V1Mode::MultiFile { files } => files.iter().map(|file| file.length as u64).sum(),
        }
    }

    fn is_private(&self) -> bool {
        self.private
    }

    fn files(&self) -> Vec<EmbededFile> {
        match &self.mode {
            V1Mode::SingleFile { length, md5sum } => vec![EmbededFile {
                length: *length,
                path: vec![self.info.name.clone()],
                md5sum: md5sum.clone(),
            }],
            V1Mode::MultiFile { files } => files.clone(),
        }
    }

    fn web_seeds(&self) -> Option<&[String]> {
        self.info.web_seeds.as_deref()
    }

    fn comment(&self) -> Option<&str> {
        self.info.comment.as_deref()
    }

    fn created_by(&self) -> Option<&str> {
        self.info.created_by.as_deref()
    }

    fn creation_date(&self) -> Option<u64> {
        self.info.creation_date
    }

    fn piece_hashes(&self) -> Vec<Vec<u8>> {
        self.pieces
            .chunks_exact(20)
            .map(|chunk| chunk.to_vec())
            .collect()
    }

    fn raw_pieces(&self) -> &[u8] {
        &self.pieces
    }
}
