use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};

use anyhow::Context;
use bqti_ipc::{Event, Reply, Request, Response, socket_path};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{
        broadcast,
        mpsc::{self, Sender},
        oneshot,
    },
};

use crate::session::SessionMode;

const EVENT_STREAM_BUFFER_SIZE: usize = 64;

pub enum IpcCommand {
    AddTorrent {
        mode: SessionMode,
        reply: oneshot::Sender<Result<String, String>>,
    },
}

struct ClientCtx {
    event_tx: broadcast::Sender<Event>,
    cmd_tx: Sender<IpcCommand>,
}

pub struct IpcServer {
    socket_path: PathBuf,
    event_tx: broadcast::Sender<Event>,
}

impl IpcServer {
    pub async fn bind() -> anyhow::Result<(Self, mpsc::Receiver<IpcCommand>)> {
        let path = socket_path().context("failed to get socket path")?;
        let _ = fs::remove_file(&path);

        let listener = UnixListener::bind(&path).context("failed to bind socket")?;

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .context("failed to set socket permissions")?;

        info!("IPC socket bound at {:?}", path);

        let (event_tx, _) = broadcast::channel(EVENT_STREAM_BUFFER_SIZE);
        let (cmd_tx, cmd_rx) = mpsc::channel(32);

        let ctx = Arc::new(ClientCtx {
            event_tx: event_tx.clone(),
            cmd_tx,
        });

        tokio::spawn(accept_loop(listener, ctx));

        Ok((
            Self {
                socket_path: path,
                event_tx,
            },
            cmd_rx,
        ))
    }

    pub fn send_event(&self, event: Event) {
        let _ = self.event_tx.send(event);
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
        info!("IPC socket removed");
    }
}

async fn accept_loop(listener: UnixListener, ctx: Arc<ClientCtx>) {
    loop {
        match listener.accept().await {
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

async fn handle_client(stream: UnixStream, ctx: Arc<ClientCtx>) -> anyhow::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut buf = String::new();

    loop {
        buf.clear();

        let n = reader
            .read_line(&mut buf)
            .await
            .context("error reading request")?;

        if n == 0 {
            return Ok(());
        }

        let request: Result<Request, _> = serde_json::from_str(buf.trim());
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
            continue;
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

    // TODO: send current state snapshot here
    // let snapshot = ctx.state.read().unwrap();
    // for event in snapshot.replicate() { ... }

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
    let (reply_tx, reply_rx) = oneshot::channel();

    match request {
        Request::Status => Ok(Response::Status(bqti_ipc::DaemonStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_torrents: 0,
            upload_rate: 0,
            download_rate: 0,
        })),
        Request::AddDownload { source } => {
            ctx.cmd_tx
                .send(IpcCommand::AddTorrent {
                    mode: SessionMode::Download {
                        target_dir: source.into(),
                    },
                    reply: reply_tx,
                })
                .await
                .map_err(|_| "daemon unavailable".to_string())?;

            let info_hash = reply_rx
                .await
                .map_err(|_| "daemon dropped the request".to_string())??;

            Ok(Response::TorrentAdded { info_hash })
        }
        Request::AddSeed { source } => {
            ctx.cmd_tx
                .send(IpcCommand::AddTorrent {
                    mode: SessionMode::Seed {
                        source_dir: source.into(),
                    },
                    reply: reply_tx,
                })
                .await
                .map_err(|_| "daemon unavailable".to_string())?;

            let info_hash = reply_rx
                .await
                .map_err(|_| "daemon dropped the request".to_string())??;

            Ok(Response::TorrentAdded { info_hash })
        }
        Request::RemoveTorrent { info_hash: _ } => Ok(Response::Handled),
        Request::Torrents => Ok(Response::Torrents(vec![])),
        Request::EventStream => Ok(Response::Handled),
        Request::Shutdown => {
            info!("shutdown requested via IPC");
            std::process::exit(0);
        }
    }
}
