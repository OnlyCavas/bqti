use std::collections::VecDeque;

use crate::dht::{Key, Node};

#[derive(Debug)]
pub struct KBucket {
    nodes: VecDeque<Node>,
    bucket_size: usize,
    pub depth: usize,
}

impl KBucket {
    pub fn new(bucket_size: usize, depth: usize) -> Self {
        Self {
            nodes: VecDeque::with_capacity(bucket_size),
            bucket_size,
            depth,
        }
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.nodes.iter().any(|n| n.id == *key)
    }

    pub fn is_full(&self) -> bool {
        self.nodes.len() == self.bucket_size
    }

    pub fn envict_and_insert(&mut self, node: Node) {
        self.nodes.pop_front();
        self.insert(node);
    }

    pub fn insert(&mut self, node: Node) {
        if let Some(node_pos) = self.nodes.iter().position(|n| n.id == node.id) {
            self.nodes.remove(node_pos);
            self.nodes.push_back(node);
            return;
        }

        if self.nodes.len() >= self.bucket_size {
            return;
        }

        self.nodes.push_back(node);
    }

    pub fn remove(&mut self, key: &Key) {
        if let Some(pos) = self.nodes.iter().position(|n| &n.id == key) {
            self.nodes.remove(pos);
        }
    }

    pub fn get_oldest_node(&self) -> Option<&Node> {
        self.nodes.front()
    }

    pub fn split(&mut self) -> (KBucket, KBucket) {
        let next_depth = self.depth + 1;

        let mut left_bucket = KBucket::new(next_depth, self.bucket_size);
        let mut right_bucket = KBucket::new(next_depth, self.bucket_size);

        for node in self.nodes.drain(..) {
            if Self::get_bit(&node.id, self.depth) {
                right_bucket.insert(node);
            } else {
                left_bucket.insert(node);
            }
        }

        (left_bucket, right_bucket)
    }

    fn get_bit(id: &Key, bit_index: usize) -> bool {
        let byte_index = bit_index / 8;
        let bit_offset = 7 - (bit_index % 8);

        if byte_index >= id.id().len() {
            return false;
        }

        (id.id()[byte_index] >> bit_offset) & 1 == 1
    }

    pub fn get_nodes(&self) -> impl Iterator<Item = &Node> {
        return self.nodes.iter();
    }
}
