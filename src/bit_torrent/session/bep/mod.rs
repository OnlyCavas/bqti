use std::sync::Arc;

mod message;
mod pipeline;
mod router;

pub use message::{BepId, StandardMessage, StandardMessageError};
pub use pipeline::{BLOCK_SIZE, Pipeline};
pub use router::{BepRouter, BepRouterError};

#[derive(Debug)]
pub(crate) enum PeerState {
    Connecting,
    Pending { peer_id: BepId, we_initiated: bool },
    Active(Arc<Pipeline>),
}
