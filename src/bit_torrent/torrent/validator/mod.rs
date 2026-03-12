use crate::bit_torrent::torrent::torrent::{TorrentError, TorrentFile};

mod v1;

pub fn validate(torrent: &TorrentFile) -> Result<(), TorrentError> {
    match torrent {
        TorrentFile::V1(torrent_v1) => v1::validate(torrent_v1),
        TorrentFile::V2(_torrent_v2) => todo!(),
    }
}
