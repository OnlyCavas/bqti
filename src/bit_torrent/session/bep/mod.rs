use std::sync::Arc;
use tokio::time::Instant;

mod message;
mod pipeline;
mod router;

pub use message::{BepId, StandardMessage, StandardMessageError};
pub use pipeline::{BLOCK_SIZE, Pipeline};
pub use router::{BepRouter, BepRouterError};

#[derive(Debug)]
pub(crate) enum PeerState {
    Pending {
        peer_id: BepId,
        initiated: Instant,
        we_initiated: bool,
    },
    Active(Arc<Pipeline>),
}
