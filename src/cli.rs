use clap::{Parser, Subcommand, ValueEnum, ValueHint};
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
        #[arg(value_parser = parse_addr)]
        addr: String,
    },

    #[command(flatten)]
    Daemon(Daemon),

    Certs {
        #[arg(value_enum)]
        kind: CertType,

        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    Torrent {
        #[command(subcommand)]
        torrent: Torrent,
    },
}

#[derive(Subcommand)]
pub enum Daemon {
    Download {
        #[arg(value_name = "TORRENT", value_hint = ValueHint::AnyPath)]
        torrent: String,

        #[arg(value_name = "OUTPUT")]
        output: String,
    },
    Seed {
        #[arg(value_name = "PATH")]
        path: String,

        #[arg(short = 't', long = "tracker", num_args = 1.., value_parser = parse_tier)]
        announce: Vec<Vec<String>>,

        #[arg(short = 's', long = "seeds", num_args = 1..)]
        seeds: Option<Vec<String>>,

        #[arg(long = "bootstrap", num_args = 1..)]
        nodes: Option<Vec<String>>,

        #[arg(short = 'l', long = "length", default_value_t = 524288)]
        piece_length: u64,

        #[arg(short, long)]
        private: bool,

        #[arg(short, long)]
        comment: Option<String>,

        #[arg(short = 'b', long = "by")]
        created_by: Option<String>,
    },
    Remove {
        #[arg(value_name = "INFO_HASH", value_hint = ValueHint::AnyPath)]
        info_hash: String,
    },
    Status,
}

fn parse_addr(addr: &str) -> Result<String, String> {
    if addr.starts_with(":") {
        return Ok(format!("127.0.0.1{}", addr));
    }

    if addr.starts_with("localhost") {
        return Ok(addr.replace("localhost", "127.0.0.1"));
    }

    Ok(addr.to_string())
}

#[derive(Debug, Clone, ValueEnum)]
pub enum CertType {
    Root,
    Leaf,
}

#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum TorrentVersion {
    #[default]
    V1,
    V2,
    Hybrid,
}

#[derive(Subcommand)]
pub enum Torrent {
    Create {
        path: PathBuf,

        #[arg(short, long)]
        name: Option<String>,

        #[arg(short, long, num_args = 1..)]
        files: Vec<PathBuf>,

        #[arg(long, value_enum, default_value_t = TorrentVersion::V1)]
        version: TorrentVersion,

        #[arg(short = 't', long = "tracker", num_args = 1.., value_parser = parse_tier)]
        announce: Vec<Vec<String>>,

        #[arg(short = 's', long = "seeds", num_args = 1..)]
        seeds: Option<Vec<String>>,

        #[arg(long = "bootstrap", num_args = 1..)]
        nodes: Option<Vec<String>>,

        #[arg(short = 'l', long = "length", default_value_t = 524288)]
        piece_length: u64,

        #[arg(short, long)]
        private: bool,

        #[arg(short, long)]
        comment: Option<String>,

        #[arg(short = 'b', long = "by")]
        created_by: Option<String>,

        #[arg(short, long)]
        output: Option<String>,
    },
    Inspect {
        #[arg(value_hint = ValueHint::FilePath)]
        torrent: PathBuf,
    },
    Validate {
        #[arg(value_hint = ValueHint::FilePath)]
        torrent: PathBuf,
    },
}

fn parse_tier(s: &str) -> Result<Vec<String>, String> {
    Ok(s.split(',').map(String::from).collect())
}
