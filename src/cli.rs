use clap::{Parser, Subcommand, ValueHint};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "BQTI - BitTorrent+QUIC+TEE+I2P")]
pub struct Cli {
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub subcommand: Option<SubCommand>,
}

#[derive(Subcommand)]
pub enum SubCommand {
    Torrent {
        #[command(subcommand)]
        torrent: Torrent,
    },
}

#[derive(Subcommand)]
pub enum Torrent {
    Inspect {
        #[arg(value_hint = ValueHint::FilePath)]
        torrent: PathBuf,
    },
    Validate {
        #[arg(value_hint = ValueHint::FilePath)]
        torrent: PathBuf,
    },
}
