use std::path::PathBuf;

use crate::{BQTIError, BitTorrentError, bit_torrent::torrent::validator, load, utils};

pub fn inspect(torrent: PathBuf, verbose: bool) -> Result<(), BQTIError> {
    let torrent_path = torrent.to_str().ok_or(BitTorrentError::InvalidPath())?;

    match load(&torrent_path) {
        Ok(torrent) => Ok(utils::print_torrent(&torrent, verbose)),
        Err(e) => panic!("{0}", e.to_string()),
    }
}

pub fn validate(torrent: PathBuf) -> Result<(), BQTIError> {
    let torrent_path = torrent.to_str().ok_or(BitTorrentError::InvalidPath())?;

    let validate_torrent = match load(&torrent_path) {
        Ok(torrent) => validator::validate(&torrent)
            .map_err(|e| BQTIError::BitTorrent(BitTorrentError::Torrent(e))),
        Err(e) => panic!("{0}", e.to_string()),
    };

    match validate_torrent {
        Ok(_) => {
            println!("torrent is valid");
            Ok(())
        }
        Err(e) => Err(e),
    }
}
