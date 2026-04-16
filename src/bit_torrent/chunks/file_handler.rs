use std::{
    fs::{File, OpenOptions},
    io::{self},
    marker::PhantomData,
    os::unix::fs::FileExt,
    path::Path,
    sync::Arc,
};

use async_trait::async_trait;
use thiserror::Error;

use crate::{
    bit_torrent::{
        chunks::{FileChunk, Size},
        torrent::metainfo::{PieceLength, v1::EmbededFile},
    },
    utils::bqti::{ensure_parent_dirs, preallocate},
};

#[derive(Debug, Error)]
pub enum ChunkHandlerError {
    #[error(transparent)]
    IoError(#[from] io::Error),

    #[error("failed to read chunk")]
    ThreadFailed(),

    #[error("index {0} out of bounds")]
    OutOfBounds(u32),
}

#[async_trait]
pub trait Reader {
    async fn read_piece(&self, index: u32) -> Result<Vec<u8>, ChunkHandlerError>;
}

#[async_trait]
pub trait Writer {
    async fn write_piece(&self, index: u32, data: Vec<u8>) -> Result<(), ChunkHandlerError>;
}

#[derive(Debug)]
pub struct Downloading;

#[derive(Debug)]
pub struct Seeding;

#[derive(Debug)]
pub struct MultiFileHandler<State> {
    targets: Vec<FileChunk>,
    piece_length: Size,
    total_length: Size,
    _state: PhantomData<State>,
}

impl MultiFileHandler<Seeding> {
    pub async fn seed(
        base_path: &Path,
        piece_length: PieceLength,
        files: &[EmbededFile],
    ) -> Result<Self, ChunkHandlerError> {
        let mut targets = Vec::new();
        let mut current_offset = 0u64;

        for embedded in files {
            let full_path = if base_path.is_file() {
                base_path.to_path_buf()
            } else {
                base_path.join(embedded.to_path())
            };

            debug!("loading: {}", full_path.to_string_lossy());

            let file = File::open(&full_path)?;
            let length = file.metadata()?.len();

            targets.push(FileChunk::new(file, current_offset, length));
            current_offset += length;
        }

        let handler = Self {
            targets,
            piece_length: piece_length.value(),
            total_length: current_offset,
            _state: PhantomData,
        };

        Ok(handler)
    }
}

impl MultiFileHandler<Downloading> {
    pub async fn download(
        base_path: &Path,
        piece_length: PieceLength,
        files: &[EmbededFile],
    ) -> Result<Self, ChunkHandlerError> {
        let mut targets = Vec::new();
        let mut current_offset = 0u64;

        for embedded in files {
            let full_path = if base_path.is_file() {
                base_path.to_path_buf()
            } else {
                base_path.join(embedded.to_path())
            };

            debug!("allocating: {}", full_path.to_string_lossy());
            ensure_parent_dirs(&full_path)?;

            let file_opts = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&full_path)?;

            let length = embedded.length as u64;
            preallocate(&file_opts, length)?;

            targets.push(FileChunk::new(file_opts, current_offset, length));
            current_offset += length;
        }

        let handler = Self {
            targets,
            piece_length: piece_length.value(),
            total_length: current_offset,
            _state: PhantomData,
        };

        Ok(handler)
    }
}

struct PieceOverlap {
    file_offset: u64,
    buffer_offset: usize,
    size: usize,
}

impl<S> MultiFileHandler<S> {
    fn piece_overlap(
        target_start: u64,
        target_length: u64,
        piece_start: u64,
        piece_end: u64,
    ) -> Option<PieceOverlap> {
        let file_end = target_start + target_length;

        if target_start >= piece_end || file_end <= piece_start {
            return None;
        }

        let overlap_start = target_start.max(piece_start);
        let overlap_end = piece_end.min(file_end);

        let overlap = PieceOverlap {
            file_offset: overlap_start - target_start,
            buffer_offset: (overlap_start - piece_start) as usize,
            size: (overlap_end - overlap_start) as usize,
        };

        Some(overlap)
    }
}

#[async_trait]
impl Reader for MultiFileHandler<Seeding> {
    async fn read_piece(&self, index: u32) -> Result<Vec<u8>, ChunkHandlerError> {
        let piece_start = index as Size * self.piece_length;
        let piece_end = (piece_start + self.piece_length).min(self.total_length);

        if piece_start >= self.total_length {
            return Err(ChunkHandlerError::OutOfBounds(index));
        }

        let mut buffer = vec![0u8; (piece_end - piece_start) as usize];

        for target in &self.targets {
            let Some(overlap) =
                Self::piece_overlap(target.start_byte, target.length, piece_start, piece_end)
            else {
                continue;
            };

            let handle = target.handle.clone();
            let chunk = tokio::task::spawn_blocking(move || {
                let mut buf = vec![0u8; overlap.size];
                handle.read_at(&mut buf, overlap.file_offset)?;
                Ok::<Vec<u8>, ChunkHandlerError>(buf)
            })
            .await
            .map_err(|_| ChunkHandlerError::ThreadFailed())??;

            buffer[overlap.buffer_offset..overlap.buffer_offset + overlap.size]
                .copy_from_slice(&chunk);
        }

        Ok(buffer)
    }
}

#[async_trait]
impl Writer for MultiFileHandler<Downloading> {
    async fn write_piece(&self, index: u32, data: Vec<u8>) -> Result<(), ChunkHandlerError> {
        let piece_start = index as Size * self.piece_length;
        let piece_end = piece_start + data.len() as u64;

        if piece_start >= self.total_length {
            return Err(ChunkHandlerError::OutOfBounds(index));
        }

        let data = Arc::new(data);

        for target in &self.targets {
            let Some(overlap) =
                Self::piece_overlap(target.start_byte, target.length, piece_start, piece_end)
            else {
                continue;
            };

            let handle = target.handle.clone();
            let data = data.clone();

            tokio::task::spawn_blocking(move || {
                handle.write_at(
                    &data[overlap.buffer_offset..overlap.buffer_offset + overlap.size],
                    overlap.file_offset,
                )?;

                Ok::<(), io::Error>(())
            })
            .await
            .map_err(|_| ChunkHandlerError::ThreadFailed())??;
        }

        Ok(())
    }
}
