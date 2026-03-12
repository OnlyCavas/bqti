use crate::bit_torrent::torrent::torrent::{TorrentError, TorrentV1};

pub(crate) fn validate(torrent: &TorrentV1) -> Result<(), TorrentError> {
    validate_piece_legth(torrent)?;
    validate_pieces(torrent)?;
    // TODO validate mode (file or single)

    Ok(())
}

fn validate_piece_legth(torrent: &TorrentV1) -> Result<(), TorrentError> {
    let pl = torrent.info.piece_length;

    const MIN_PIECE_SIZE: i64 = 16 * 1024; // 16 KB
    const MAX_PIECE_SIZE: i64 = 256 * 1024 * 1024; // 256 MB

    if pl < MIN_PIECE_SIZE || pl > MAX_PIECE_SIZE || (pl & (pl - 1)) != 0 {
        return Err(TorrentError::NotValid(
            "piece length must be a power of two between 16KB and 256MB".into(),
        ));
    }

    Ok(())
}

fn validate_pieces(torrent: &TorrentV1) -> Result<(), TorrentError> {
    let hashes = torrent.hashes()?;
    let hash_length = hashes.len();

    if hash_length == 0 || hash_length % 20 != 0 {
        return Err(TorrentError::NotValid(
            format!("Pieces length ({}) is not a multiple of 20", hash_length).into(),
        ));
    }

    if let Some(first) = hashes.get(0) {
        println!("Primeiro hash (Hex): {:?}", hex::encode(first));
    }

    Ok(())
}
