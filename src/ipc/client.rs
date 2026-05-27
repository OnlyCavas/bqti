use std::io;

use ::console::style;
use anyhow::Context;
use bqti_ipc::{Event, Request, Response, Socket, Torrent, TorrentState};
use futures::{Stream, StreamExt};
use indicatif::ProgressBar;
use tokio::pin;

use crate::{
    cli::{Daemon, DownloadCommand},
    utils::console,
};

fn get_pwd(link: &str) -> anyhow::Result<String> {
    let pwd = std::env::current_dir()?
        .join(link)
        .to_string_lossy()
        .into_owned();

    Ok(pwd)
}

async fn ipc(request: Request) -> anyhow::Result<Response> {
    let mut socket = Socket::connect()
        .await
        .context("could not connect to bqti daemon — is it running?")?;

    socket
        .send(request)
        .await
        .context("error communicating with bqti")?
        .map_err(|e| anyhow::anyhow!(e).context("bqti returned an error"))
}

async fn event_stream() -> anyhow::Result<impl Stream<Item = io::Result<Event>>> {
    let mut socket = Socket::connect()
        .await
        .context("could not connect to bqti daemon — is it running?")?;

    socket
        .send(Request::EventStream)
        .await
        .context("error communicating with bqti")?
        .map_err(|e| anyhow::anyhow!(e).context("bqti returned an error"))?;

    Ok(socket.event_stream())
}

async fn watch_single(info_hash: String) -> anyhow::Result<()> {
    let event_stream = event_stream().await?;
    pin!(event_stream);

    let mut pb: Option<ProgressBar> = None;

    while let Some(event) = event_stream.next().await {
        let event = event.context("error reading event")?;

        match event {
            Event::SessionStateChanged {
                info_hash: ref hash,
                ref name,
                ref state,
            } if hash == &info_hash => match state {
                TorrentState::Verifying { verified, total } => {
                    let pb = pb.get_or_insert_with(|| console::verify_pieces_bar(*total, name));
                    pb.set_position(*verified as u64);

                    if verified == total {
                        pb.finish_with_message(format!("verified {}", style(name).bold().green()));
                        break;
                    }
                }
                _ => {
                    if let Some(pb) = pb.take() {
                        pb.finish_and_clear();
                    }
                }
            },
            Event::ExposeTorrent {
                ref info_hash,
                ref magnet,
            } => {
                if let Some(pb) = pb.take() {
                    pb.finish_and_clear();
                }

                console::expose_torrent(info_hash, magnet);
                break;
            }
            _ => (),
        }
    }

    Ok(())
}

pub async fn handle_client(cli: Daemon) -> anyhow::Result<()> {
    match cli {
        Daemon::Download { command } => handle_download(command).await,
        Daemon::Seed {
            path,
            name,
            announce,
            seeds,
            nodes,
            piece_length,
            private,
            comment,
            created_by,
        } => {
            handle_seed(
                path,
                name,
                announce,
                seeds,
                nodes,
                piece_length,
                private,
                comment,
                created_by,
            )
            .await
        }
        Daemon::Status { info_hash } => handle_status(info_hash).await,
    }
}

async fn handle_download(command: DownloadCommand) -> anyhow::Result<()> {
    match command {
        DownloadCommand::Add { torrent, watch } => {
            let link = resolve_link(&torrent)?;
            let user_space = std::env::current_dir()?.to_string_lossy().into();

            let response = ipc(Request::AddDownload { link, user_space }).await?;

            let Response::TorrentAdded { info_hash } = response else {
                return Ok(());
            };

            console::print_queue(&info_hash);

            if watch {
                handle_status(Some(info_hash)).await?;
            }
        }
        DownloadCommand::Stop { info_hash } => {
            ipc(Request::RemoveTorrent {
                info_hash: info_hash.clone(),
            })
            .await?;

            console::print_removed(&info_hash);
        }
        DownloadCommand::Pause { info_hash } => {
            ipc(Request::PauseSession {
                info_hash: info_hash.clone(),
            })
            .await?;

            console::print_paused(&info_hash);
        }
        DownloadCommand::Resume { info_hash } => {
            ipc(Request::ResumeSession {
                info_hash: info_hash.clone(),
            })
            .await?;

            console::print_resumed(&info_hash);
        }
        DownloadCommand::List => {
            let response = ipc(Request::Torrents).await?;
            if let Response::Torrents(torrents) = response {
                if torrents.is_empty() {
                    println!("  {} no active torrents", style("·").dim());
                    return Ok(());
                }

                for torrent in torrents {
                    console::print_torrent(&torrent);
                }
            }
        }
    };

    Ok(())
}

fn resolve_link(torrent: &str) -> anyhow::Result<String> {
    if torrent.starts_with("magnet:") {
        Ok(torrent.to_string())
    } else {
        get_pwd(torrent)
    }
}

async fn handle_seed(
    path: String,
    name: Option<String>,
    announce: Vec<Vec<String>>,
    seeds: Option<Vec<String>>,
    nodes: Option<Vec<String>>,
    piece_length: u64,
    private: bool,
    comment: Option<String>,
    created_by: Option<String>,
) -> anyhow::Result<()> {
    let path = get_pwd(&path)?;

    let display_name = name
        .as_deref()
        .unwrap_or_else(|| path.split('/').last().unwrap_or(&path));

    let sp = console::spinner(&format!(
        "generating .torrent for {}",
        style(display_name).bold().green()
    ));

    let response = ipc(Request::AddSeed {
        name,
        path,
        piece_length,
        announce,
        seeds,
        nodes,
        private,
        comment,
        created_by,
    })
    .await?;

    sp.finish_and_clear();

    let Response::SeedAdded { info_hash } = response else {
        anyhow::bail!("unexpected response");
    };

    watch_single(info_hash).await
}

async fn handle_status(info_hash: Option<String>) -> anyhow::Result<()> {
    let response = ipc(Request::Torrents).await?;

    let Response::Torrents(mut torrents) = response else {
        anyhow::bail!("unexpected response");
    };

    if let Some(ref hash) = info_hash {
        torrents.retain(|t| &t.info_hash == hash);

        if torrents.is_empty() {
            anyhow::bail!("torrent {} not found", style(hash).bold());
        }
    }

    let stream = event_stream().await?;
    pin!(stream);

    console::print_torrent_list(&torrents);

    while let Some(event) = stream.next().await {
        let event = event.context("error reading event")?;

        let relevant = |hash: &str| info_hash.as_deref().map_or(true, |h| h == hash);

        match event {
            Event::TorrentRemoved { ref info_hash } if relevant(info_hash) => {
                torrents.retain(|t| &t.info_hash != info_hash);
                console::print_torrent_list(&torrents);
            }
            Event::TorrentAdded {
                ref info_hash,
                ref name,
                ref state,
            } if relevant(info_hash) => {
                torrents.push(Torrent {
                    info_hash: info_hash.clone(),
                    name: name.clone(),
                    state: state.clone(),
                });

                console::print_torrent_list(&torrents);
            }
            Event::SessionStateChanged {
                ref info_hash,
                state,
                ..
            } if relevant(info_hash) => {
                if let Some(t) = torrents.iter_mut().find(|t| &t.info_hash == info_hash) {
                    t.state = state;
                }

                console::print_torrent_list(&torrents);
            }
            _ => (),
        }
    }

    Ok(())
}
