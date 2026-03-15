use std::path::PathBuf;

use serde_bytes::ByteBuf;

use crate::{
    BQTIError, BitTorrentError,
    bit_torrent::torrent::{
        builder::TorrentBuilder,
        metainfo::{
            Integrity, Metainfo, TorrentError, TorrentFile,
            v1::{EmbededFile, V1Mode},
        },
    },
    cli::{CreateArgs, TorrentVersion},
    load, save, utils,
};

pub fn create(args: CreateArgs) -> Result<(), BQTIError> {
    // TODO generate pieces and hashes, depending the torrent version

    let builder = match args.version {
        TorrentVersion::V1 => TorrentBuilder::with_v1(
            "my torrent".into(),
            1,
            ByteBuf::new(),
            V1Mode::SingleFile {
                file: EmbededFile {
                    path: vec!["".into()],
                    length: 0,
                    md5sum: None,
                },
            },
        )
        .build(),
        TorrentVersion::V2 => todo!(),
        TorrentVersion::Hybrid => todo!(),
    };

    let torrent: TorrentFile = builder.map_err(|e| BitTorrentError::Torrent(e))?;

    let mut final_path = args
        .output
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if final_path.is_dir() {
        let clean_name = torrent
            .name()
            .trim()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_");

        let filename = format!("{}.torrent", clean_name);
        final_path.push(filename);
    }

    let path_str = final_path.to_str().ok_or_else(|| {
        BitTorrentError::Torrent(TorrentError::Failed("Caminho UTF-8 inválido".into()))
    })?;

    save(path_str, &torrent)?;
    println!("Torrent criado com sucesso em: {}", path_str);
    Ok(())
}

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
