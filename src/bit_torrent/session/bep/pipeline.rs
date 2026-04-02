use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, Ordering},
};

use tokio::sync::RwLock;

use crate::{
    dht::Node,
    session::{bit_field::BitField, session::TorrentSession},
};

pub(crate) const MAX_PIPELINE: usize = 5;
pub const BLOCK_SIZE: u32 = 16384;

#[derive(Clone, Debug)]
pub struct BlockRequest {
    pub index: u32,
    pub begin: u32,
    pub length: u32,
}

#[derive(Debug)]
pub struct Pipeline {
    pub peer: Node,
    bitfield: RwLock<BitField>,

    am_choking: AtomicBool, // NOTE: active when choking
    am_interested: AtomicBool,
    peer_choking: AtomicBool,
    peer_interested: AtomicBool,

    pending: RwLock<VecDeque<BlockRequest>>,
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
            pending: RwLock::new(VecDeque::new()),
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

    pub fn on_choke(&self) {
        self.peer_choking.store(true, Ordering::Relaxed);

        if let Ok(mut pending) = self.pending.try_write() {
            pending.clear();
        }
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
        let mut pending = self.pending.write().await;
        pending.retain(|r| r.index != index);
    }

    pub async fn set_bitfield(&self, index: usize) {
        let mut bitfield = self.bitfield.write().await;
        bitfield.set(index);
    }

    pub async fn fill_requests(
        &self,
        received: Option<(u32, u32)>,
        session: &TorrentSession,
        piece_size: impl Fn(u32) -> u32,
    ) -> Vec<BlockRequest> {
        if self.is_choked() {
            return vec![];
        }

        let peer_bitfield = self.bitfield.read().await;
        let session_bitfield = session.get_bitfield().await;

        let mut pending = self.pending.write().await;

        if let Some((index, begin)) = received {
            pending.retain(|r| !(r.index == index && r.begin == begin));
        }

        let max_blocks = peer_bitfield
            .difference(&session_bitfield)
            .next()
            .map(|i| piece_size(i as u32).div_ceil(BLOCK_SIZE) as usize)
            .unwrap_or(MAX_PIPELINE);

        let max_pipeline = MAX_PIPELINE.max(max_blocks);
        let mut available_slots: usize = max_pipeline.saturating_sub(pending.len());

        if available_slots == 0 {
            return vec![];
        }

        let mut new_requests = Vec::new();

        for piece_index in peer_bitfield.difference(&session_bitfield) {
            let piece_index = piece_index as u32;
            let actual_size = piece_size(piece_index);
            let mut current_begin = 0;

            while current_begin < actual_size {
                let block_len = (actual_size - current_begin).min(BLOCK_SIZE);
                let already_pending = pending
                    .iter()
                    .any(|r| r.index == piece_index && r.begin == current_begin);

                if !already_pending {
                    let request = BlockRequest {
                        index: piece_index,
                        begin: current_begin,
                        length: block_len,
                    };

                    pending.push_back(request.clone());
                    new_requests.push(request);

                    available_slots -= 1;
                }

                if available_slots == 0 {
                    return new_requests;
                }

                current_begin += BLOCK_SIZE;
            }
        }

        new_requests
    }

    pub async fn on_piece_received(&self, index: u32, begin: u32) {
        let mut pending = self.pending.write().await;

        pending.retain(|r| !(r.index == index && r.begin == begin));
    }
}
