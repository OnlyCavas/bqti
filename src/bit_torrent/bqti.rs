use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc::{self};
use tokio_util::sync::CancellationToken;

use crate::{
    BitTorrentError,
    bit_torrent::{
        certs::{KeyIdentity, PublicKey},
        magnet::MagnetLink,
        pex::{PexMessage, PexRouter},
        torrent::metainfo::TorrentError,
    },
    dht::{DhtPacket, Kademlia, RpcHandler, TorrentDht},
    ipc::server::{IpcCommandError, IpcServer},
    load,
    network::{ConnectionManager, Message, Packet},
    session::{BepRouter, SessionManager, SessionMode, StandardMessage, TorrentSessionError},
    torrent::{
        builder::TorrentBuilder,
        metainfo::{InfoHash, Integrity, TorrentFile},
    },
};

#[derive(Debug, Error)]
pub enum BqtiTorretingError {
    #[error(transparent)]
    TorrentSessionError(#[from] TorrentSessionError),

    #[error(transparent)]
    BitTorrentError(#[from] BitTorrentError),

    #[error(transparent)]
    TorrentError(#[from] TorrentError),

    #[error("failed to load .torrent file")]
    FailedToLoad(),

    #[error(transparent)]
    IpcCommandError(#[from] IpcCommandError),

    #[error("unsupported feature: {0}")]
    Unsupported(String),
}

pub struct Bqti {
    connection_manager: Arc<ConnectionManager>,
    kademlia: Arc<Kademlia>,
    rpc_handler: Arc<RpcHandler>,
    pex: Arc<PexRouter>,
    torrenting_session: Arc<SessionManager>,
}

pub struct SeedingOptions {
    pub path: PathBuf,
    pub piece_length: u64,
    pub announce: Vec<Vec<String>>,
    pub seeds: Option<Vec<String>>,
    pub nodes: Option<Vec<String>>,
    pub private: bool,
    pub comment: Option<String>,
    pub created_by: Option<String>,
}

pub enum TorrentAction {
    Download { source: TorrentSource },
    Seed { options: SeedingOptions },
}

pub enum TorrentSource {
    MagnetLink(MagnetLink),
    TorrentFile(PathBuf),
}

impl TorrentSource {
    pub fn parse(link: &str) -> Result<Self, String> {
        if link.starts_with("magnet:") {
            return MagnetLink::new(link)
                .map(TorrentSource::MagnetLink)
                .ok_or_else(|| "invalid magnet link".to_string());
        }

        Ok(TorrentSource::TorrentFile(link.into()))
    }
}

#[async_trait]
pub trait Torrenting {
    async fn add_torrent(&self, action: TorrentAction) -> Result<TorrentFile, BqtiTorretingError>;
    async fn remove_torrent(&self, info_hash: InfoHash) -> Result<InfoHash, BqtiTorretingError>;
}

#[async_trait]
impl Torrenting for Bqti {
    async fn add_torrent(&self, action: TorrentAction) -> Result<TorrentFile, BqtiTorretingError> {
        let (torrent_file, mode) = match action {
            TorrentAction::Download { source } => {
                let torrent = match source {
                    TorrentSource::TorrentFile(torrent_path) => load(torrent_path)?,
                    TorrentSource::MagnetLink(_) => {
                        return Err(BqtiTorretingError::Unsupported("magnet links".into()));
                    }
                };

                (
                    torrent,
                    SessionMode::Download {
                        target_dir: "may be error here".into(),
                    },
                )
            }

            TorrentAction::Seed { options } => {
                let file_name = options
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or(BqtiTorretingError::FailedToLoad())?;

                let torrent =
                    TorrentBuilder::with_v1(file_name, &options.path, options.piece_length as i64)
                        .file(options.path.clone())
                        .announce_list(options.announce)
                        .dht_nodes(options.nodes)
                        .web_seeds(options.seeds)
                        .comment(options.comment)
                        .private(options.private)
                        .created_by(options.created_by)
                        .build()?;

                (
                    torrent,
                    SessionMode::Seed {
                        source_dir: options.path,
                    },
                )
            }
        };

        torrent_file.validate()?;
        self.torrenting_session.add(mode, &torrent_file).await;

        Ok(torrent_file)
    }

    async fn remove_torrent(&self, info_hash: InfoHash) -> Result<InfoHash, BqtiTorretingError> {
        self.torrenting_session.remove(&info_hash).await;
        Ok(info_hash)
    }
}

impl Bqti {
    pub fn new(
        connection_manager: Arc<ConnectionManager>,
        certificate: KeyIdentity,
    ) -> Result<Arc<Self>> {
        let kad_cert = certificate.leaf("dht certificate", true)?;
        let bep_cert = certificate.leaf("bep certificate", true)?;

        let rpc_handler = Arc::new(RpcHandler::new(connection_manager.clone()));
        let kademlia = Kademlia::new(rpc_handler.clone(), kad_cert)?;

        let pex_router = PexRouter::new(connection_manager.clone());
        let bep_router = BepRouter::new(
            bep_cert.pub_key(),
            connection_manager.clone(),
            kademlia.clone(),
            pex_router.clone(),
        )?;

        let manager = Arc::new(SessionManager::new(bep_router));

        let bqti = Arc::new(Self {
            kademlia,
            pex: pex_router,
            connection_manager,
            rpc_handler,
            torrenting_session: manager,
        });

        Ok(bqti)
    }

    pub async fn serve_forever(
        self: &Arc<Bqti>,
        mut stream_rx: mpsc::Receiver<Packet>,
    ) -> anyhow::Result<()> {
        let mut join_set = tokio::task::JoinSet::new();
        let cancellation_token = CancellationToken::new();

        let manager = self.connection_manager.clone();
        let cancel_tx = cancellation_token.clone();

        // FIX: is missing passing thru the state
        let (ipc_server, _ipc_state) = IpcServer::start(self.clone())
            .await
            .context("failed to start")?;

        join_set.spawn(async move {
            manager.start_listening(cancel_tx).await;
        });

        loop {
            tokio::select! {
                Some(Packet(message, source_addr)) = stream_rx.recv() => {
                    let bqti = Arc::clone(&self);

                    join_set.spawn(async move {
                        match message {
                            Message::KeepAlive => info!("keep alive"),
                            Message::DHT(payload) => {
                                match DhtPacket::from_bytes(&payload) {
                                    Ok(packet) => {
                                        let Some(request) = bqti.rpc_handler.dispatch(packet).await else {
                                            return;
                                        };

                                        let _ = bqti.kademlia.handle_packet(request, source_addr).await;
                                    },
                                    Err(e) => error!("dht parse error: {}", e),
                                }
                            },
                            Message::PEX(payload) => {
                                match PexMessage::from_bytes(&payload) {
                                    Ok(pex) => {
                                        let pex_handler = bqti.pex.clone();

                                        let _ = pex_handler.handle_incoming(pex, &source_addr, move |info_hash, socket| {
                                            let kademlia = bqti.kademlia.clone();

                                            async move {
                                                let _ = kademlia.announce_peer(info_hash, socket).await;
                                            }
                                        }).await;
                                    },
                                    Err(e) => error!("pex parse error: {}", e),
                                }
                            },
                            Message::Standard(payload) => {
                                match StandardMessage::from_bytes(&payload) {
                                    Ok(msg) => {
                                        bqti.torrenting_session.dispatch(msg, source_addr).await;
                                    }
                                    Err(e) => error!("standard message parse error: {}", e),
                                }
                            }
                        }
                    });

                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }

        cancellation_token.cancel();
        self.connection_manager.shutdown().await;
        join_set.shutdown().await;

        drop(ipc_server);

        Ok(())
    }
}
