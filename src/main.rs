use bqti::bit_torrent::{
    bencode,
    torrent::torrent::{TorrentFile, TorrentMode},
};
use chrono::{DateTime, Utc};

fn print_torrent(torrent: TorrentFile, all: bool) {
    let divider = "─".repeat(60);
    let thin = "·".repeat(60);

    println!("┌{}┐", divider);
    println!("│{:^60}│", "🧲 TORRENT INFO");
    println!("└{}┘", divider);
    println!();

    println!("  🔑 Info Hash    {}", hex::encode(torrent.info_hash()));
    println!("  {}", thin);

    if let Some(announce) = torrent.announce() {
        println!("  📡 Announce     {}", announce);
        println!("");
    }

    if let Some(announce_list) = torrent.announce_list() {
        println!("  📡 Trackers");
        for (i, tier) in announce_list.iter().enumerate() {
            println!("     Tier {}", i + 1);
            for tracker in tier {
                println!("       ↳ {}", tracker);
            }
        }
    }

    if let Some(web_seeds) = torrent.web_seeds() {
        println!("  🌐 Web Seeds");
        for url in web_seeds {
            println!("       ↳ {}", url);
        }
    }

    println!();
    println!("  {}", thin);
    println!("  📦 Name         {}", torrent.name());
    println!("  🧩 Piece Length {}", format_size(torrent.piece_length()));

    match torrent.mode() {
        TorrentMode::SingleFile { .. } => {
            println!("  📄 Mode         Single File");
        }
        TorrentMode::MultiFile { files } => {
            println!("  📁 Mode         Multi File ({} files)", files.len());
            for file in files {
                println!(
                    "       ↳ {} — {}",
                    file.path.join("/"),
                    format_size(file.length)
                );
            }
        }
    }

    println!(
        "  💾 Total Size   {}",
        format_size(torrent.total_size().unwrap_or_default())
    );

    println!();
    println!("  {}", thin);

    if let Ok(hashes) = torrent.piece_hashes() {
        println!("  🔢 Pieces       {}", hashes.len());

        if all {
            println!();
            println!("  {}", thin);
            println!("  🔍 Piece Hashes");
            for (i, hash) in hashes.iter().enumerate() {
                println!("     [{:>6}] {}", i, hex::encode(hash));
            }
        }
    }

    println!();
    println!("  {}", thin);
    println!("  💬 Version      {:?}", torrent.version());

    if let Some(comment) = torrent.comment() {
        println!("  💬 Comment      {}", comment);
    }

    if let Some(created_by) = torrent.created_by() {
        println!("  🛠  Created By   {}", created_by);
    }

    if let Some(creation_date) = torrent.creation_date() {
        let dt = DateTime::<Utc>::from_timestamp(creation_date, 0)
            .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| creation_date.to_string());

        println!("  📅 Created On   {}", dt);
    }

    println!();
    println!("  {}", "─".repeat(60));
}

pub fn format_size(bytes: i64) -> String {
    match bytes {
        b if b >= 1024 * 1024 * 1024 => format!("{:.2} GB", b as f64 / (1024.0 * 1024.0 * 1024.0)),
        b if b >= 1024 * 1024 => format!("{:.2} MB", b as f64 / (1024.0 * 1024.0)),
        b if b >= 1024 => format!("{:.2} KB", b as f64 / 1024.0),
        b => format!("{} B", b),
    }
}

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        panic!("Is missing an argument")
    };

    match bencode::decode(&path) {
        Ok(torrent) => print_torrent(torrent, false),
        Err(e) => panic!("{0}", e.to_string()),
    }
}
