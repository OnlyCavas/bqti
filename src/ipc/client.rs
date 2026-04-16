use anyhow::{Context, Ok};
use bqti_ipc::{Request, Response, Socket};

use crate::cli::Daemon;

fn get_pwd(link: &str) -> anyhow::Result<String> {
    let pwd = std::env::current_dir()?
        .join(link)
        .to_string_lossy()
        .into_owned();

    Ok(pwd)
}

pub async fn handle_client(cli: Daemon) -> anyhow::Result<()> {
    let request: Request = match cli {
        Daemon::Download { torrent, output: _ } => {
            let link = if torrent.starts_with("magnet:") {
                torrent
            } else {
                get_pwd(&torrent)?
            };

            Request::AddDownload { link }
        }
        Daemon::Seed {
            path,
            announce,
            seeds,
            nodes,
            piece_length,
            private,
            comment,
            created_by,
        } => {
            let path = get_pwd(&path)?;

            Request::AddSeed {
                path,
                piece_length,
                announce,
                seeds,
                nodes,
                private,
                comment,
                created_by,
            }
        }
        Daemon::Remove { info_hash } => Request::RemoveTorrent { info_hash },
        Daemon::Status => Request::Status,
    };

    handle_incoming(ipc(request).await?)
}

fn handle_incoming(response: Response) -> anyhow::Result<()> {
    match response {
        Response::TorrentAdded { info_hash } => {
            info!("{} queued", info_hash);

            Ok(())
        }
        Response::SeedingStarted {
            info_hash,
            magnet_link,
        } => {
            info!("{} queued", info_hash);
            info!("magnet link: {}", magnet_link);

            Ok(())
        }
        Response::Removed { info_hash } => {
            info!("{} removed", info_hash);

            Ok(())
        }
        Response::Status(status) => {
            info!("{}", status);

            Ok(())
        }
        unexpected => anyhow::bail!("unexpected response: {unexpected:?}"),
    }
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
