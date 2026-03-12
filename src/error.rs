use thiserror::Error;

use crate::BitTorrentError;

#[derive(Error, Debug)]
pub enum BQTIError {
    #[error(transparent)]
    BitTorrent(#[from] BitTorrentError),
}
