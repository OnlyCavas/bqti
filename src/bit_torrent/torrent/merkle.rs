use sha2::{Digest, Sha256};

use crate::{bit_torrent::torrent::metainfo::TorrentError, types::MerkleRoot};

pub struct MerkleTree {
    pub root: MerkleRoot,
}

impl MerkleTree {
    pub fn from_piece_layers(bytes: &[u8]) -> Result<Self, TorrentError> {
        if bytes.is_empty() {
            return Err(TorrentError::NotValid("Piece layer data is empty".into()));
        }

        if bytes.len() % 32 != 0 {
            return Err(TorrentError::NotValid(format!(
                "Piece layer length ({}) is not a multiple of 32",
                bytes.len()
            )));
        }

        let mut hashes = Vec::with_capacity(bytes.len() / 32);

        for chunk in bytes.chunks_exact(32) {
            let array: [u8; 32] = chunk
                .try_into()
                .map_err(|_| TorrentError::NotValid("Failed to parse 32-byte hash chunk".into()))?;

            hashes.push(array);
        }

        Ok(Self::from_piece_hashes(&hashes))
    }

    pub fn from_piece_hashes(hashes: &[[u8; 32]]) -> Self {
        if hashes.is_empty() {
            return Self { root: [0u8; 32] };
        }

        let mut current_level = hashes.to_vec();

        while current_level.len() > 1 {
            let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

            for chunk in current_level.chunks(2) {
                let mut hasher = Sha256::new();
                hasher.update(&chunk[0]);

                if chunk.len() == 2 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update([0u8; 32]);
                }

                next_level.push(hasher.finalize().into());
            }

            current_level = next_level;
        }

        Self {
            root: current_level[0],
        }
    }
}
