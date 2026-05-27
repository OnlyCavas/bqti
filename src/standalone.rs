use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::{
    BQTIError, BitTorrentError,
    bit_torrent::torrent::{
        builder::TorrentBuilder,
        metainfo::{Integrity, Metainfo, TorrentError, TorrentFile},
    },
    cli::{CertType, Torrent, TorrentVersion},
    load, save,
    utils::{self, bqti::certs_dir},
};

fn file_name(args: &CreateArgs) -> Result<&str, BQTIError> {
    if let Some(name) = &args.name {
        return Ok(name);
    }

    let Some(Some(file_name)) = args.path.file_name().map(|name| name.to_str()) else {
        return Err(BQTIError::BitTorrent(BitTorrentError::InvalidPath()));
    };

    Ok(file_name)
}

pub struct CreateArgs {
    pub path: PathBuf,
    pub name: Option<String>,
    pub files: Vec<PathBuf>,
    pub version: TorrentVersion,
    pub announce: Vec<Vec<String>>,
    pub seeds: Option<Vec<String>>,
    pub nodes: Option<Vec<String>>,
    pub piece_length: u64,
    pub private: bool,
    pub comment: Option<String>,
    pub created_by: Option<String>,
    pub output: Option<String>,
}

pub fn handle_torrent(torrent: Torrent, verbose: bool) -> Result<()> {
    match torrent {
        Torrent::Create {
            path,
            name,
            files,
            version,
            announce,
            seeds,
            nodes,
            piece_length,
            private,
            comment,
            created_by,
            output,
        } => create(CreateArgs {
            path,
            name,
            files,
            version,
            announce,
            seeds,
            nodes,
            piece_length,
            private,
            comment,
            created_by,
            output,
        }),
        Torrent::Inspect { torrent } => inspect(torrent, verbose),
        Torrent::Validate { torrent } => validate(torrent),
    }
}

fn create(args: CreateArgs) -> Result<()> {
    let builder = match args.version {
        TorrentVersion::V1 => {
            TorrentBuilder::with_v1(file_name(&args)?, &args.path, args.piece_length as i64)
                .file(args.path)
                .files(args.files)
                .announce_list(args.announce)
                .dht_nodes(args.nodes)
                .web_seeds(args.seeds)
                .comment(args.comment)
                .private(args.private)
                .created_by(args.created_by)
                .build()
        }
        TorrentVersion::V2 => {
            TorrentBuilder::with_v2(file_name(&args)?, &args.path, args.piece_length as i64)
                .file(args.path)
                .files(args.files)
                .announce_list(args.announce)
                .web_seeds(args.seeds)
                .dht_nodes(args.nodes)
                .comment(args.comment)
                .created_by(args.created_by)
                .build()
        }
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
    info!("torrent created at: {}", path_str);
    Ok(())
}

fn inspect(torrent: PathBuf, verbose: bool) -> Result<()> {
    let torrent_path = torrent.to_str().ok_or(BitTorrentError::InvalidPath())?;

    match load(&torrent_path) {
        Ok(torrent) => Ok(utils::print_torrent(&torrent, verbose)),
        Err(e) => panic!("{0}", e.to_string()),
    }
}

fn validate(torrent: PathBuf) -> Result<()> {
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

pub async fn handle_certs(_kind: CertType, _output: Option<PathBuf>) -> Result<()> {
    let dir = match _output {
        Some(ref path) => PathBuf::from(path),
        None => certs_dir().context("cannot determine certs directory")?,
    };

    #[cfg(feature = "tee")]
    {
        use crate::{
            certs::TeeKeyIdentity,
            utils::certs::{CertOptions, store_cert},
        };

        let enclave_identity = TeeKeyIdentity::new()?;

        store_cert(&enclave_identity, &CertOptions::default()).await?;
        utils::console::print_certs(&_kind, &dir, &enclave_identity, None);

        return Ok(());
    }

    #[cfg(not(feature = "tee"))]
    {
        use crate::certs::KeyIdentity;
        use crate::utils::certs::CertOptions;
        use crate::utils::certs::store_cert;

        let ca_root = utils::certs::make_ca_root().await?;

        match _kind {
            CertType::Root => {
                store_cert(&ca_root, &CertOptions::new(&dir)).await?;

                utils::console::print_certs(&_kind, &dir, &ca_root, None);
            }
            CertType::Leaf => {
                let leaf = ca_root.leaf("localhost", true)?;

                store_cert(&ca_root, &CertOptions::new(&dir)).await?;
                store_cert(&leaf, &CertOptions::new(&dir)).await?;

                utils::console::print_certs(&_kind, &dir, &ca_root, Some(&leaf));
            }
        }

        Ok(())
    }
}
