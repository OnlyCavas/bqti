use std::{fs::File, sync::Arc};

mod assembler;
mod file_handler;

type Size = u64;

#[derive(Debug)]
pub struct FileChunk {
    handle: Arc<File>,
    start_byte: Size,
    length: Size,
}

impl FileChunk {
    pub fn new(handle: File, start_byte: Size, length: Size) -> Self {
        Self {
            handle: Arc::new(handle),
            start_byte,
            length,
        }
    }
}

pub use assembler::PieceAssembler;
pub use file_handler::{ChunkHandlerError, Downloading, MultiFileHandler, Reader, Seeding, Writer};
