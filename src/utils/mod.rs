use chrono::{DateTime, Utc};

use crate::bit_torrent::torrent::metainfo::Metainfo;

pub mod bqti;
pub mod certs;
pub mod console;

pub fn print_torrent(torrent: &impl Metainfo, all: bool) {
    let divider = "─".repeat(60);
    let thin = "+".repeat(59);

    println!("┌{}┐", divider);
    println!("│{:^59}│", "🧲 TORRENT INFO");
    println!("└{}┘", divider);
    println!();

    println!(
        "  🔑 Info Hash    {}",
        hex::encode(torrent.info_hash().as_ref())
    );

    println!();
    println!("  {}", thin);
    println!();

    if let Some(announce) = torrent.announce() {
        println!("  📡 Announce     {}", announce);
        println!("");
    }

    if let Some(dht_nodes) = torrent.dht_nodes() {
        println!("  📡 BootStrap nodes");

        for bootstrap in dht_nodes {
            println!("       ↳ {}", bootstrap.to_string());
        }

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
    println!();
    println!("  📦 Name         {}", torrent.name());
    println!(
        "  🧩 Piece Length {}",
        format_size(torrent.piece_length().0)
    );

    let files = torrent.files();
    match files.len() {
        0 => println!("  📄 Mode        Error No File Found"),

        1 => println!("  📄 Mode         Single File"),
        _ => {
            println!("  📁 Mode         Multi File ({} files)", files.len());
            for file in files {
                println!(
                    "       ↳ {} — {}",
                    file.path.join("/"),
                    format_size(file.length as u64)
                );
            }
        }
    }

    println!();
    println!("  💾 Total Size   {}", format_size(torrent.total_size()));

    println!();
    println!("  {}", thin);

    let hashes = torrent.piece_hashes();

    println!();
    println!("  🔢 Pieces       {}", hashes.len());

    if all {
        println!();
        println!("  {}", thin);
        println!("  🔍 Piece Hashes");
        for (i, hash) in hashes.iter().enumerate() {
            println!("     [{:>6}] {}", i, hex::encode(hash));
        }
    }

    println!();
    println!("  {}", thin);

    println!();
    println!("  💬 Version      {:?}", torrent.version());

    if let Some(comment) = torrent.comment() {
        println!("  💬 Comment      {}", comment);
    }

    if let Some(created_by) = torrent.created_by() {
        println!("  🛠  Created By   {}", created_by);
    }

    if let Some(creation_date) = torrent.creation_date() {
        let dt = DateTime::<Utc>::from_timestamp(creation_date as i64, 0)
            .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| creation_date.to_string());

        println!("  📅 Created On   {}", dt);
    }

    println!();
    println!("  {}", "─".repeat(60));
}

pub fn format_size(bytes: u64) -> String {
    let bytes_f = bytes as f64;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes_f / 1073741824.0)
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes_f / 1048576.0)
    } else if bytes >= 1024 {
        format!("{:.2} KB", bytes_f / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}
