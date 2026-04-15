use anyhow::{Context, Ok};
use bqti_ipc::{Request, Response, Socket};

use crate::cli::Daemon;

type DeamonHandler = fn(Response) -> anyhow::Result<()>;

pub async fn handle_client(cli: Daemon) -> anyhow::Result<()> {
    let (request, handler): (Request, DeamonHandler) = match cli {
        Daemon::Download { torrent, output: _ } => (
            Request::AddDownload { source: torrent },
            handle_torrent_added,
        ),
        Daemon::Seed { path } => (Request::AddDownload { source: path }, handle_torrent_added),
        Daemon::Status => (Request::Status, handle_status),
    };

    let response = ipc(request).await?;

    handler(response)
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

fn handle_torrent_added(response: Response) -> anyhow::Result<()> {
    match response {
        Response::TorrentAdded { info_hash } => {
            info!("{} queued", info_hash);

            Ok(())
        }
        unexpected => anyhow::bail!("unexpected response: {unexpected:?}"),
    }
}

fn handle_status(response: Response) -> anyhow::Result<()> {
    match response {
        Response::Status(status) => {
            info!("{}", status);

            Ok(())
        }
        unexpected => anyhow::bail!("unexpected response: {unexpected:?}"),
    }
}
