use std::path::PathBuf;

use crate::{
    BQTIError, BitTorrentError, bit_torrent::torrent::metainfo::TorrentIntegrity, load, utils,
};

pub fn inspect(torrent: PathBuf, verbose: bool) -> Result<(), BQTIError> {
    let torrent_path = torrent.to_str().ok_or(BitTorrentError::InvalidPath())?;

    match load(&torrent_path) {
        Ok(torrent) => Ok(utils::print_torrent(&torrent, verbose)),
        Err(e) => panic!("{0}", e.to_string()),
    }
}

pub fn validate(torrent: PathBuf) -> Result<(), BQTIError> {
    let torrent_path = torrent.to_str().ok_or(BitTorrentError::InvalidPath())?;
    let torrent = load(&torrent_path)?;

    match torrent.validate() {
        Ok(_) => {
            println!(".torrent metadata file is valid!");
            Ok(())
        }
        Err(e) => Err(BQTIError::BitTorrent(BitTorrentError::Torrent(e))),
    }
}
