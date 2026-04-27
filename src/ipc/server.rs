use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};

use anyhow::Context;
use bqti_ipc::{
    Event, Reply, Request, Response, TorrentState, socket_path,
    state::{EventStream, IpcState},
};

use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{RwLock, broadcast, oneshot},
};
use tokio_util::sync::CancellationToken;

use crate::{
    SeedingOptions, TorrentAction, TorrentSource, Torrenting,
    session::SessionMode,
    torrent::metainfo::{InfoHash, Metainfo},
};

const EVENT_STREAM_BUFFER_SIZE: usize = 64;

#[derive(Debug, Error)]
pub enum IpcCommandError {
    #[error("failed to add torrent file to download queue")]
    AddTorrentError(),
}

pub type TorrentingHandle = Arc<dyn Torrenting + Send + Sync>;

pub enum IpcCommand {
    AddTorrent {
        mode: SessionMode,
        reply: oneshot::Sender<Result<InfoHash, IpcCommandError>>,
    },
}

struct ClientCtx {
    event_tx: broadcast::Sender<Event>,
    bqti: TorrentingHandle,
    state: Arc<RwLock<IpcState>>,
    cancellation_token: CancellationToken,
}

impl ClientCtx {
    async fn emit(&self, event: Event) {
        let event = {
            let mut state = self.state.write().await;
            state.apply(event)
        };

        if let Some(event) = event {
            let _ = self.event_tx.send(event);
        }
    }
}

pub struct IpcServer {
    socket_path: PathBuf,
    event_tx: broadcast::Sender<Event>,
    state: Arc<RwLock<IpcState>>,
}

impl IpcServer {
    pub async fn start(bqti: TorrentingHandle) -> anyhow::Result<Arc<Self>> {
        let path = socket_path();
        let cancellation_token = CancellationToken::new();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("failed to create socket directory")?;
        }

        let _ = fs::remove_file(&path);

        let listener = UnixListener::bind(&path).context("failed to bind socket")?;

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .context("failed to set socket permissions")?;

        info!("IPC socket bound at {:?}", path);

        let (event_tx, _) = broadcast::channel(EVENT_STREAM_BUFFER_SIZE);

        let state = Arc::new(RwLock::new(IpcState::default()));

        let ctx = Arc::new(ClientCtx {
            event_tx: event_tx.clone(),
            state: state.clone(),
            bqti,
            cancellation_token: cancellation_token.clone(),
        });

        tokio::spawn(accept_loop(listener, ctx, cancellation_token));

        let ipc_server = Arc::new(Self {
            socket_path: path,
            event_tx,
            state,
        });

        Ok(ipc_server)
    }

    pub async fn send_event(&self, event: Event) {
        let event = {
            let mut state = self.state.write().await;
            state.apply(event)
        };

        if let Some(event) = event {
            let _ = self.event_tx.send(event);
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
        info!("IPC socket removed");
    }
}

async fn accept_loop(
    listener: UnixListener,
    ctx: Arc<ClientCtx>,
    cancellation_token: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancellation_token.cancelled() => {

            },
            stream = listener.accept() => {
                match stream {
                    Ok((stream, _)) => {
                        debug!("new IPC client connected");
                        let ctx = ctx.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_client(stream, ctx).await {
                                warn!("IPC client error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        error!("IPC accept error: {e}");
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_client(stream: UnixStream, ctx: Arc<ClientCtx>) -> anyhow::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);

    loop {
        let mut buf = Vec::new();
        let response = reader.read_until(b'\n', &mut buf).await;

        match response {
            Ok(0) => return Ok(()),
            Ok(_) => (),
            Err(err) => return Err(err).context("error reading request"),
        }

        let request: Result<Request, _> = serde_json::from_slice(&buf);
        let is_event_stream = matches!(request, Ok(Request::EventStream));

        let reply: Reply = match request {
            Ok(req) => dispatch(req, &ctx).await,
            Err(e) => Err(format!("invalid request: {e}")),
        };

        let mut out = serde_json::to_string(&reply).context("error serializing reply")?;
        out.push('\n');
        write
            .write_all(out.as_bytes())
            .await
            .context("error writing reply")?;

        if !is_event_stream {
            return Ok(());
        }

        debug!("client upgrading to event stream");
        handle_event_stream(&mut write, &ctx).await?;

        return Ok(());
    }
}

async fn handle_event_stream(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    ctx: &ClientCtx,
) -> anyhow::Result<()> {
    let mut rx = ctx.event_tx.subscribe();

    {
        let state = ctx.state.read().await;

        for event in state.replicate() {
            let mut line =
                serde_json::to_string(&event).context("error serializing snapshot event")?;

            line.push('\n');

            write
                .write_all(line.as_bytes())
                .await
                .context("error writing snapshot")?;
        }
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                let mut line = serde_json::to_string(&event).context("error serializing event")?;
                line.push('\n');

                if let Err(e) = write.write_all(line.as_bytes()).await {
                    if e.kind() == std::io::ErrorKind::BrokenPipe {
                        debug!("event stream client disconnected");
                        return Ok(());
                    }

                    return Err(e).context("error writing event");
                }
            }
            Err(broadcast::error::RecvError::Closed) => {
                debug!("event channel closed, dropping client");
                return Ok(());
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("event stream client lagged, missed {n} events");

                let reply: Reply = Err(format!("lagged: missed {n} events"));
                let mut line = serde_json::to_string(&reply).unwrap();
                line.push('\n');

                let _ = write.write_all(line.as_bytes()).await;

                return Ok(());
            }
        }
    }
}

async fn dispatch(request: Request, ctx: &ClientCtx) -> Reply {
    match request {
        Request::Status { info_hash: _ } => Ok(Response::Status(bqti_ipc::DaemonStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_torrents: 0,
            upload_rate: 0,
            download_rate: 0,
        })),
        Request::AddDownload { link } => {
            let torrent_file = ctx
                .bqti
                .add_torrent(TorrentAction::Download {
                    source: TorrentSource::parse(&link)?,
                })
                .await
                .map_err(|e| e.to_string())?;

            let info_hash = torrent_file.info_hash().to_string();

            ctx.emit(Event::TorrentAdded {
                info_hash: info_hash.clone(),
                name: torrent_file.name().to_string(),
                state: TorrentState::Pending,
            })
            .await;

            Ok(Response::TorrentAdded { info_hash })
        }
        Request::AddSeed {
            path,
            piece_length,
            announce,
            seeds,
            nodes,
            private,
            comment,
            created_by,
            name,
        } => {
            let torrent_file = ctx
                .bqti
                .add_torrent(TorrentAction::Seed {
                    options: SeedingOptions {
                        path: path.into(),
                        announce,
                        seeds,
                        nodes,
                        piece_length,
                        private,
                        comment,
                        created_by,
                        name,
                    },
                })
                .await
                .map_err(|e| e.to_string())?;

            let info_hash = torrent_file.info_hash().to_string();

            ctx.emit(Event::TorrentAdded {
                info_hash: info_hash.clone(),
                name: torrent_file.name().to_string(),
                state: TorrentState::Pending,
            })
            .await;

            Ok(Response::SeedAdded { info_hash })
        }
        Request::RemoveTorrent { info_hash } => {
            let info_hash = InfoHash::from_hex(&info_hash).ok_or("info hash invalid")?;

            let info_hash = ctx
                .bqti
                .remove_torrent(info_hash)
                .await
                .map_err(|e| e.to_string())?;

            ctx.emit(Event::TorrentRemoved {
                info_hash: info_hash.to_string(),
            })
            .await;

            Ok(Response::Removed {
                info_hash: info_hash.to_string(),
            })
        }

        Request::ResumeSession { info_hash } => {
            let info_hash = InfoHash::from_hex(&info_hash).ok_or("info hash invalid")?;

            ctx.bqti
                .resume_torrent(info_hash)
                .await
                .map_err(|e| e.to_string())?;

            Ok(Response::Handled)
        }

        Request::PauseSession { info_hash } => {
            let info_hash = InfoHash::from_hex(&info_hash).ok_or("info hash invalid")?;
            let hash = info_hash.to_string();

            ctx.bqti
                .pause_torrent(info_hash)
                .await
                .map_err(|e| e.to_string())?;

            ctx.emit(Event::SessionStateChanged {
                info_hash: hash,
                name: "".into(),
                state: TorrentState::Paused,
            })
            .await;

            Ok(Response::Handled)
        }

        Request::Torrents => {
            let current_torrents = {
                let state = ctx.state.read().await;
                state.active_torrents.values().cloned().collect::<Vec<_>>()
            };

            Ok(Response::Torrents(current_torrents))
        }

        Request::Shutdown => {
            info!("shutdown requested via IPC");
            ctx.cancellation_token.cancel();
            Ok(Response::Handled)
        }

        Request::EventStream => Ok(Response::Handled),
    }
}
