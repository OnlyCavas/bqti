use crate::session::BLOCK_SIZE;

pub struct PieceAssembler {
    total_size: u32,
    received_size: u32,
    buffer: Vec<u8>,
    received_mask: Vec<bool>,
}

impl PieceAssembler {
    pub fn new(_index: u32, total_size: u32) -> Self {
        let block_count = total_size.div_ceil(BLOCK_SIZE) as usize;

        Self {
            total_size,
            received_size: 0,
            buffer: vec![0u8; total_size as usize],
            received_mask: vec![false; block_count],
        }
    }

    pub fn add_block(&mut self, begin: u32, data: &[u8]) -> bool {
        let block_idx = (begin / BLOCK_SIZE) as usize;

        if block_idx >= self.received_mask.len() {
            return false;
        }

        if self.received_mask[block_idx] {
            return self.is_complete();
        }

        let end = (begin as usize + data.len()).min(self.buffer.len());
        self.buffer[begin as usize..end].copy_from_slice(&data[..end - begin as usize]);
        self.received_mask[block_idx] = true;
        self.received_size += data.len() as u32;

        self.is_complete()
    }

    pub fn has_block(&self, begin: u32) -> bool {
        let block_idx = (begin / BLOCK_SIZE) as usize;
        self.received_mask.get(block_idx).copied().unwrap_or(false)
    }

    pub fn is_complete(&self) -> bool {
        self.received_size >= self.total_size
    }

    pub fn assemble(self) -> Vec<u8> {
        self.buffer
    }
}
