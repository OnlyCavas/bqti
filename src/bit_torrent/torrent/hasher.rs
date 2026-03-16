use crossbeam::channel::{self, Sender};
use sha1::{Digest, Sha1};
use std::{fs::File, io::Read, path::PathBuf};

use crate::{
    bit_torrent::torrent::metainfo::{
        TorrentError,
        v1::{EmbededFile, V1Mode},
    },
    types::PieceByte,
};

pub trait PieceHasher {
    type Output;

    fn file(&mut self, path: impl Into<PathBuf>) -> &mut Self;

    fn finalize(self) -> Result<Self::Output, TorrentError>;
}

type HashEntry = (usize, Vec<u8>);

pub struct PieceHasherV1 {
    piece_length: usize,
    files: Vec<PathBuf>,
}

impl PieceHasherV1 {
    pub fn new(piece_length: usize) -> Self {
        Self {
            piece_length,
            files: vec![],
        }
    }
}

impl PieceHasher for PieceHasherV1 {
    type Output = (PieceByte, V1Mode);

    fn file(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.files.push(path.into());
        self
    }

    fn finalize(self) -> Result<Self::Output, TorrentError> {
        let mut results: Vec<(usize, [u8; 20])>;
        let mut files: Vec<EmbededFile>;
        let (piece_tx, piece_rx) = channel::bounded::<HashEntry>(16); // read file data
        let (meta_tx, meta_rx) = channel::unbounded::<EmbededFile>(); // read file metadata

        (results, files) = std::thread::scope(|s| {
            s.spawn(move || file_handle(&self.files, &self.piece_length, &meta_tx, &piece_tx));

            let calculated_hashes = piece_rx
                .into_iter()
                .map(|(idx, data)| {
                    let mut h = Sha1::new();
                    h.update(&data);
                    (idx, h.finalize().into())
                })
                .collect();

            let files: Vec<EmbededFile> = meta_rx.into_iter().collect();

            (calculated_hashes, files)
        });

        results.sort_by_key(|k| k.0);
        let hashes: Vec<u8> = results.into_iter().flat_map(|(_, h)| h).collect();

        let mode = if files.len() == 1 {
            let Some(file) = files.pop() else {
                return Err(TorrentError::Failed("couldn't get the file".into()));
            };

            V1Mode::SingleFile { file: file }
        } else {
            V1Mode::MultiFile { files }
        };

        Ok((serde_bytes::ByteBuf::from(hashes), mode))
    }
}

fn file_handle(
    files: &[PathBuf],
    piece_length: &usize,
    meta_tx: &Sender<EmbededFile>,
    tx: &Sender<HashEntry>,
) -> Result<(), TorrentError> {
    let mut piece_index = 0;
    let mut buffer = vec![0u8; *piece_length];
    let mut bytes_buffered = 0;

    for path in files.iter() {
        let file = dispatch_file_metadata(meta_tx, path)?;
        let mut reader = std::io::BufReader::new(file);

        loop {
            let position = reader.read(&mut buffer[bytes_buffered..]).unwrap_or(0);

            if position == 0 {
                break;
            }

            bytes_buffered += position;

            if bytes_buffered == *piece_length {
                tx.send((piece_index, buffer.clone())).ok();
                buffer = vec![0u8; *piece_length];
                piece_index += 1;
                bytes_buffered = 0;
            }
        }
    }

    // flush remaining pieces
    if bytes_buffered > 0 {
        buffer.truncate(bytes_buffered);
        tx.send((piece_index, buffer)).ok();
    }

    Ok(())
}

fn dispatch_file_metadata(
    meta_tx: &Sender<EmbededFile>,
    path: &PathBuf,
) -> Result<File, TorrentError> {
    let Ok(file) = File::open(&path) else {
        return Err(TorrentError::Failed("failed to open file".into()));
    };

    let Ok(metadata) = file.metadata() else {
        return Err(TorrentError::Failed("failed get file metadata".into()));
    };

    let Some(file_path) = path.to_str() else {
        return Err(TorrentError::Failed("failed to parse file path".into()));
    };

    meta_tx
        .send(EmbededFile {
            length: metadata.len() as i64,
            path: vec![file_path.to_string()],
            md5sum: None,
        })
        .ok();

    Ok(file)
}
