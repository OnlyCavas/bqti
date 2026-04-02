use thiserror::Error;

use crate::bit_torrent::chunks::ChunkHandlerError;

mod bep;
mod bit_field;
mod manager;
mod resume;
mod session;
mod state;
mod transition;

#[derive(Debug, Error)]
pub enum TorrentSessionError {
    #[error("failed to load .torrent file")]
    FailedToLoad(),

    #[error("wrong state")]
    StateError(),

    #[error("unable to find the home directory")]
    UnableToFindXDGFolder(),

    #[error(transparent)]
    ChunkReaderError(#[from] ChunkHandlerError),
}

pub use bep::{
    BLOCK_SIZE, BepId, BepRouter, BepRouterError, StandardMessage, StandardMessageError,
};
pub use bit_field::BitField;
pub use manager::SessionManager;
pub use session::SessionMode;
