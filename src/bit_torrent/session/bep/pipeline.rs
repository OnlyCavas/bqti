use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

use tokio::sync::RwLock;

use crate::{
    dht::Node,
    session::{bit_field::BitField, session::TorrentSession},
};

pub(crate) const MAX_PIPELINE: usize = 128;
pub const BLOCK_SIZE: u32 = 16384;

#[derive(Clone, Debug)]
pub struct BlockRequest {
    pub index: u32,
}

#[derive(Debug)]
pub struct Pipeline {
    pub peer: Node,
    bitfield: RwLock<BitField>,
    am_choking: AtomicBool,
    am_interested: AtomicBool,
    peer_choking: AtomicBool,
    peer_interested: AtomicBool,
    pending: RwLock<HashSet<u32>>,
}

impl Pipeline {
    pub fn new(peer: Node, bitfield: BitField) -> Self {
        Self {
            peer,
            bitfield: RwLock::new(bitfield),
            am_choking: AtomicBool::new(true),
            am_interested: AtomicBool::new(false),
            peer_choking: AtomicBool::new(true),
            peer_interested: AtomicBool::new(false),
            pending: RwLock::new(HashSet::new()),
        }
    }

    pub async fn evaluate_interest(&self, our_bitfield: &BitField) -> bool {
        let peer_bf = self.bitfield.read().await;
        let interested = peer_bf.difference(our_bitfield).next().is_some();

        self.am_interested.store(interested, Ordering::Relaxed);
        interested
    }

    pub fn on_unchoke(&self) {
        self.peer_choking.store(false, Ordering::Relaxed);
    }

    pub async fn on_choke(&self) {
        self.peer_choking.store(true, Ordering::Relaxed);
        self.pending.write().await.clear();
        self.am_interested.store(false, Ordering::Relaxed);
    }

    pub fn unchoke(&self) {
        self.am_choking.store(false, Ordering::Relaxed);
    }

    pub fn choke(&self) {
        self.am_choking.store(true, Ordering::Relaxed);
    }

    pub fn we_are_choking(&self) -> bool {
        self.am_choking.load(Ordering::Relaxed)
    }

    pub fn we_are_interested(&self) -> bool {
        self.am_interested.load(Ordering::Relaxed)
    }

    pub fn on_peer_interested(&self) {
        self.peer_interested.store(true, Ordering::Relaxed);
    }

    pub fn on_peer_not_interested(&self) {
        self.peer_interested.store(false, Ordering::Relaxed);
    }

    pub fn is_choked(&self) -> bool {
        self.peer_choking.load(Ordering::Relaxed)
    }

    pub fn peer_is_interested(&self) -> bool {
        self.peer_interested.load(Ordering::Relaxed)
    }

    pub async fn clear_piece(&self, index: u32) {
        self.pending.write().await.remove(&index);
    }

    pub async fn set_bitfield(&self, index: usize) {
        let mut bitfield = self.bitfield.write().await;
        bitfield.set(index);
    }

    pub async fn fill_requests(
        &self,
        received: Option<u32>,
        session: &TorrentSession,
    ) -> Vec<BlockRequest> {
        if self.is_choked() {
            return vec![];
        }

        let peer_bitfield = self.bitfield.read().await;
        let session_bitfield = session.get_bitfield().await;
        let mut pending = self.pending.write().await;

        if let Some(index) = received {
            pending.remove(&index);
        }

        let mut available_slots = MAX_PIPELINE.saturating_sub(pending.len());

        if available_slots == 0 {
            return vec![];
        }

        let mut new_requests = Vec::new();

        for piece_index in peer_bitfield.difference(&session_bitfield) {
            let piece_index = piece_index as u32;

            if !pending.contains(&piece_index) {
                pending.insert(piece_index);
                new_requests.push(BlockRequest { index: piece_index });
                available_slots -= 1;
            }

            if available_slots == 0 {
                return new_requests;
            }
        }

        new_requests
    }
}
