use clap::Parser;

use anyhow::Result;

use crate::bit_torrent::torrent::path::TorrentPath;

#[derive(Parser, Debug)]
pub struct SeedArgs {
    #[arg(value_name = "TORRENT")]
    torrent: String,
}

pub async fn run(args: SeedArgs) -> Result<()> {
    let builder = TorrentPath::new(&args.torrent).add(args.torrent).build();

    for file in builder {
        info!(
            "File: abs -> {} : rel -> {}",
            file.0.to_string_lossy(),
            file.1.to_string_lossy()
        );
    }

    Ok(())
}
