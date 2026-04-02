#[derive(Debug, Clone)]
pub struct BitField {
    bytes: Vec<u8>,
    pub piece_count: usize,
}

impl BitField {
    pub fn empty(piece_count: usize) -> Self {
        let byte_count = piece_count.div_ceil(8);

        Self {
            bytes: vec![0u8; byte_count],
            piece_count,
        }
    }

    pub fn from_wire(bytes: Vec<u8>, piece_count: usize) -> Self {
        Self { bytes, piece_count }
    }

    pub fn have(&self, index: usize) -> bool {
        if index >= self.piece_count {
            return false;
        }

        let byte = index / 8;
        let bit = 7 - (index % 8);

        self.bytes[byte] & (1 << bit) != 0
    }

    pub fn set(&mut self, index: usize) {
        if index >= self.piece_count {
            return;
        }

        let byte = index / 8;
        let bit = 7 - (index % 8);

        self.bytes[byte] |= 1 << bit;
    }

    pub fn unset(&mut self, index: usize) {
        if index >= self.piece_count {
            return;
        }

        let byte = index / 8;
        let bit = 7 - (index % 8);

        self.bytes[byte] &= !(1 << bit);
    }

    pub fn is_complete(&self) -> bool {
        self.missing_pieces().next().is_none()
    }

    pub fn count(&self) -> usize {
        (0..self.piece_count).filter(|&i| self.have(i)).count()
    }

    pub fn missing_pieces(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.piece_count).filter(|&i| !self.have(i))
    }

    pub fn available_pieces(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.piece_count).filter(|&i| self.have(i))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn difference<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = usize> + 'a {
        let len = self.bytes.len().max(other.bytes.len());

        (0..len).flat_map(move |byte_idx| {
            let my_byte = self.bytes.get(byte_idx).copied().unwrap_or(0);
            let their_byte = other.bytes.get(byte_idx).copied().unwrap_or(0);
            let diff = my_byte & !their_byte;

            (0..8).filter_map(move |bit_pos| {
                let piece_index = byte_idx * 8 + bit_pos;

                if piece_index >= self.piece_count {
                    return None;
                }

                if (diff & (1 << (7 - bit_pos))) != 0 {
                    Some(piece_index)
                } else {
                    None
                }
            })
        })
    }
}
