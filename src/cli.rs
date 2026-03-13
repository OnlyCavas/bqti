use clap::{Args, Parser, Subcommand, ValueHint};
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

#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum TorrentVersion {
    #[default]
    V1,
    V2,
    Hybrid,
}

#[derive(Args)]
pub struct CreateArgs {
    pub path: PathBuf,

    #[arg(short = 'V', long, value_enum, default_value_t = TorrentVersion::V1)]
    pub version: TorrentVersion,

    #[arg(short, long)]
    pub announce: Option<String>,

    #[arg(short = 't', long = "tracker")]
    pub announce_list: Vec<String>,

    #[arg(short = 'l', long = "length", default_value_t = 524288)]
    pub piece_length: u64,

    #[arg(short, long)]
    pub private: bool,

    #[arg(short, long)]
    pub comment: Option<String>,

    #[arg(short = 'b', long = "by")]
    pub created_by: Option<String>,

    #[arg(short, long)]
    pub output: Option<String>,
}

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
