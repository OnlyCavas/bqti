use anyhow::Result;
use clap::Parser;

use crate::{bit_torrent::torrent::metainfo::Integrity, load};

#[derive(Parser, Debug)]
pub struct DownloadArgs {
    #[arg(value_name = "TORRENT")]
    torrent: String,

    #[arg(value_name = "OUTPUT")]
    output: String,
}

pub async fn run(args: DownloadArgs) -> Result<()> {
    // load torrent file
    let Ok(torrent_file) = load(&args.torrent) else {
        panic!("failed to load file");
    };

    // validate
    let Ok(_) = torrent_file.validate() else {
        panic!("it's invalid");
    };

    Ok(())
}
