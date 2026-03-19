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
    Serve {
        #[arg(short, long)]
        addr: String,

        #[arg(short, long)]
        to: String,
    },
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

    #[arg(short, long)]
    pub name: Option<String>,

    #[arg(short, long, num_args = 1..)]
    pub files: Vec<PathBuf>,

    #[arg(long, value_enum, default_value_t = TorrentVersion::V1)]
    pub version: TorrentVersion,

    #[arg(
        short = 't',
        long = "tracker",
        num_args = 1..,
        value_parser = parse_tier
    )]
    pub announce: Vec<Vec<String>>,

    #[arg(
        short = 's',
        long = "seeds",
        num_args = 1..,
    )]
    pub seeds: Option<Vec<String>>,

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

fn parse_tier(s: &str) -> Result<Vec<String>, String> {
    Ok(s.split(',').map(String::from).collect())
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
