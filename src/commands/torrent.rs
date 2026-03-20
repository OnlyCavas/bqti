use std::path::PathBuf;

use clap::{Subcommand, ValueHint, arg};

use anyhow::Result;

use crate::{
    BQTIError, BitTorrentError,
    bit_torrent::torrent::{
        builder::TorrentBuilder,
        metainfo::{Integrity, Metainfo, TorrentError, TorrentFile},
    },
    cli::{CreateArgs, TorrentVersion},
    load, save, utils,
};

#[derive(Subcommand)]
pub enum Torrent {
    Create(CreateArgs),
    Inspect {
        #[arg(value_hint = ValueHint::FilePath)]
        torrent: PathBuf,
    },
    Validate {
        #[arg(value_hint = ValueHint::FilePath)]
        torrent: PathBuf,
    },
}

fn file_name(args: &CreateArgs) -> Result<&str, BQTIError> {
    if let Some(name) = &args.name {
        return Ok(name);
    }

    let Some(Some(file_name)) = args.path.file_name().map(|name| name.to_str()) else {
        return Err(BQTIError::BitTorrent(BitTorrentError::InvalidPath()));
    };

    Ok(file_name)
}

pub fn create(args: CreateArgs) -> Result<()> {
    let builder = match args.version {
        TorrentVersion::V1 => TorrentBuilder::with_v1(file_name(&args)?, args.piece_length as i64)
            .file(args.path)
            .files(args.files)
            .announce_list(args.announce)
            .web_seeds(args.seeds)
            .comment(args.comment)
            .private(args.private)
            .created_by(args.created_by)
            .build(),
        TorrentVersion::V2 => TorrentBuilder::with_v2(file_name(&args)?, args.piece_length as i64)
            .file(args.path)
            .files(args.files)
            .announce_list(args.announce)
            .web_seeds(args.seeds)
            .comment(args.comment)
            .created_by(args.created_by)
            .build(),
        TorrentVersion::Hybrid => Err(BQTIError::BitTorrent(BitTorrentError::Torrent(
            TorrentError::UnsupportedVersion(0),
        )))?,
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
    info!("Torrent criado com sucesso em: {}", path_str);
    Ok(())
}

pub fn inspect(torrent: PathBuf, verbose: bool) -> Result<()> {
    let torrent_path = torrent.to_str().ok_or(BitTorrentError::InvalidPath())?;

    match load(&torrent_path) {
        Ok(torrent) => Ok(utils::print_torrent(&torrent, verbose)),
        Err(e) => panic!("{0}", e.to_string()),
    }
}

pub fn validate(torrent: PathBuf) -> Result<()> {
    let torrent_path = torrent.to_str().ok_or(BitTorrentError::InvalidPath())?;
    let torrent = load(&torrent_path)?;
    match torrent.validate() {
        Ok(_) => {
            info!(".torrent metadata file is valid!");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
