use thiserror::Error;

use crate::BitTorrentError;

#[derive(Error, Debug)]
pub enum BQTIError {
    #[error("{0}")]
    BitTorrent(#[from] BitTorrentError),
}
