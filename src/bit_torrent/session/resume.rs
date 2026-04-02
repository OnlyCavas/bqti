use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    bit_torrent::{
        bencode::{self, BencodeError},
        torrent::metainfo::InfoHash,
    },
    session::BitField,
};

const BQTI_RESUME_EXT: &str = ".bqtiresume";

#[derive(Debug, Serialize, Deserialize)]
pub struct ResumeFile {
    info_hash: Vec<u8>,
    bitfield: Vec<u8>,
    resource: String,
    total_pieces: usize,
}

impl ResumeFile {
    pub fn new(info_hash: &InfoHash, bitfield: BitField, resource: &Path) -> Self {
        Self {
            info_hash: info_hash.into(),
            bitfield: bitfield.as_bytes().into(),
            resource: resource.to_string_lossy().into_owned(),
            total_pieces: bitfield.piece_count,
        }
    }

    fn resume_path(resource: &Path) -> PathBuf {
        resource.join(BQTI_RESUME_EXT)
    }

    pub async fn open(resource: &Path, info_hash: &InfoHash) -> Option<Self> {
        let path = Self::resume_path(resource);
        let bytes = fs::read(&path).await.ok()?;
        let data: Self = bencode::decode(&bytes).ok()?;

        if &data.info_hash != info_hash.as_ref() {
            warn!("resume file info_hash mismatch, ignoring");
            return None;
        }

        Some(data)
    }

    pub async fn persist(&self, resource: &Path) -> Result<(), BencodeError> {
        let path = Self::resume_path(resource);
        let tmp = path.with_extension("bqti.tmp");
        let bytes = bencode::encode(self)?;

        fs::write(&tmp, &bytes).await?;
        fs::rename(&tmp, &path).await?;

        Ok(())
    }

    pub fn get_bitfield(&self) -> BitField {
        BitField::from_wire(self.bitfield.clone(), self.total_pieces)
    }

    pub fn is_complete(&self) -> bool {
        let bitfield = self.get_bitfield();
        (0..self.total_pieces).all(|i| bitfield.have(i))
    }
}
