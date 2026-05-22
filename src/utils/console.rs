use std::{path::Path, time::Duration};

use bqti_ipc::{Torrent, TorrentState};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};

use crate::{
    certs::{ActiveKeyIdentity, KeyIdentity},
    cli::CertType,
};

const SPINNER_TICK_DURATION: Duration = Duration::from_millis(80);

pub fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();

    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
            .template("{spinner:.bold.blue} {msg:.dim}")
            .unwrap(),
    );

    pb.set_message(msg.to_string());
    pb.enable_steady_tick(SPINNER_TICK_DURATION);
    pb
}

fn short_hash(hash: &str) -> &str {
    hash.get(..8).unwrap_or(hash)
}

pub fn print_queue(info_hash: &str) {
    println!(
        "\n  {} {}\n",
        style("✓").green().bold(),
        style("torrent queued").green()
    );

    println!(
        "    {}   {}",
        style("hash").dim(),
        style(short_hash(info_hash)).bold()
    );

    println!(
        "    {}    {}",
        style("hint").dim(),
        style("bqti status").dim()
    );
}

pub fn expose_torrent(info_hash: &str, magnet: &str) {
    println!(
        "    {}   {}",
        style("hash").dim(),
        style(short_hash(&info_hash)).bold()
    );
    println!(
        "    {}  {}",
        style("magnet").dim(),
        style(&magnet).blue().bold()
    );
}

pub fn print_removed(info_hash: &str) {
    println!(
        "\n  {} {}\n",
        style("✓").red().bold(),
        style("torrent removed").red()
    );
    println!(
        "    {}   {}\n",
        style("hash").dim(),
        style(info_hash).bold()
    );
}

pub fn print_resumed(info_hash: &str) {
    println!(
        "\n  {} {}\n",
        style("✓").green().bold(),
        style("torrent resumed").green()
    );
    println!(
        "    {}   {}\n",
        style("hash").dim(),
        style(info_hash).bold()
    );
}

pub fn print_paused(info_hash: &str) {
    println!(
        "\n  {} {}\n",
        style("✓").yellow().bold(),
        style("torrent paused").yellow()
    );
    println!(
        "    {}   {}\n",
        style("hash").dim(),
        style(info_hash).bold()
    );
}

pub fn print_torrent(torrent: &Torrent) {
    let state = match &torrent.state {
        TorrentState::Pending => style("pending").dim().to_string(),
        TorrentState::Verifying { verified, total } => {
            let pct = (*verified as f64 / *total as f64 * 100.).round() as u64;

            format!(
                "{} {}  {}%",
                style("verifying").blue(),
                format_bar(*verified as u64, *total as u64, 20),
                pct
            )
        }
        TorrentState::Downloading {
            current,
            total_pieces,
            download_rate,
        } => {
            let pct = (*current as f64 / *total_pieces as f64 * 100.).round() as u64;

            format!(
                "{} {} {}%  {} {}/s",
                style("↓").red().bold(),
                format_bar(*current as u64, *total_pieces as u64, 20),
                pct,
                style("@").dim(),
                style(format_rate(*download_rate)).blue()
            )
        }
        TorrentState::Seeding { upload_rate, peers } => format!(
            "{} {} {}/s  {} {}",
            style("↑ seeding").green().bold(),
            style("@").dim(),
            style(format_rate(*upload_rate)).green(),
            style(peers).green().bold(),
            style("peers").dim()
        ),
        TorrentState::Paused => style("⏸ paused").yellow().to_string(),
    };

    println!(
        "  {} {}  {}",
        style("·").dim(),
        style(&torrent.name).green().bold(),
        state,
    );
    println!(
        "    {}   {}\n",
        style("hash").dim(),
        style(&torrent.info_hash).dim()
    );
}

pub fn print_torrent_list(torrents: &[Torrent]) {
    print!("\x1B[2J\x1B[1;1H");

    if torrents.is_empty() {
        println!("  {} no active torrents", style("·").dim());
        return;
    }

    for torrent in torrents {
        print_torrent(torrent);
    }
}

fn format_rate(bytes_per_sec: u64) -> String {
    if bytes_per_sec >= 1_000_000 {
        format!("{:.1} MB", bytes_per_sec as f64 / 1_000_000.)
    } else if bytes_per_sec >= 1_000 {
        format!("{:.1} KB", bytes_per_sec as f64 / 1_000.)
    } else {
        format!("{bytes_per_sec} B")
    }
}

fn format_bar(current: u64, total: u64, width: usize) -> String {
    let filled = (current as f64 / total as f64 * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!(
        "{}{}{}{}{}",
        style("[").dim(),
        style("█".repeat(filled)).blue(),
        style("▓".repeat(empty.min(1))).blue().dim(),
        style("░".repeat(empty.saturating_sub(1))).dim(),
        style("]").dim(),
    )
}

pub fn verify_pieces_bar(total_pieces: u32, name: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_pieces as u64);

    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg}\n{spinner:.blue} [{bar:40.green/dim}]")
            .unwrap()
            .progress_chars("█▓░"),
    );

    pb.set_message(format!("verifying {}", style(name).bold().green()));
    pb.enable_steady_tick(SPINNER_TICK_DURATION);
    pb
}

pub fn print_certs(
    kind: &CertType,
    dir: &Path,
    identity: &ActiveKeyIdentity,
    leaf: Option<&ActiveKeyIdentity>,
) {
    use ring::digest::{SHA256, digest};

    let fingerprint = |id: &ActiveKeyIdentity| {
        let der = id.cert_der();
        let hash = digest(&SHA256, &der);
        hash.as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":")
    };

    println!(
        "\n  {} {}\n",
        style("✓").green().bold(),
        style(match kind {
            CertType::Root => "root ca generated",
            CertType::Leaf => "certificates generated",
        })
        .green()
    );

    match kind {
        CertType::Root => {
            println!(
                "    {}    {}",
                style("cert").dim(),
                style(dir.join("ca.der").display()).bold()
            );

            println!(
                "\n    {}   {}\n",
                style("fingerprint").dim(),
                style(format!("sha256:{}", fingerprint(identity))).bold()
            );

            println!(
                "    {}    {}\n",
                style("hint").dim(),
                style("distribute this to peers for mutual TLS").dim()
            );
        }
        CertType::Leaf => {
            let leaf = leaf.expect("leaf cert required for Leaf kind");

            println!(
                "    {}  {}",
                style("root ca").dim(),
                style(dir.join("ca.der").display()).bold()
            );

            println!(
                "    {}    {}",
                style("leaf").dim(),
                style(dir.join("quic.der").display()).bold()
            );

            println!(
                "    {}     {}",
                style("key").dim(),
                style(dir.join("quic_pk.der").display()).bold()
            );

            println!(
                "\n    {}   {}\n",
                style("fingerprint").dim(),
                style(format!("sha256:{}", fingerprint(leaf))).bold()
            );
        }
    }
}
