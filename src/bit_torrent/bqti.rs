use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc::{self};
use tokio_util::sync::CancellationToken;

use crate::{
    BitTorrentError,
    bit_torrent::{
        magnet::MagnetLink,
        pex::{PexMessage, PexRouter},
        torrent::metainfo::TorrentError,
    },
    certs::ActiveKeyIdentity,
    dht::{DhtPacket, Kademlia, KademliaServer, RpcHandler, TorrentDht},
    ipc::server::{IpcCommandError, IpcServer},
    load,
    network::{ConnectionManager, Message, Packet},
    session::{
        BepRouter, SessionManager, SessionManagerError, SessionMode, StandardMessage,
        TorrentSessionError,
    },
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
    SessionManagerError(#[from] SessionManagerError),

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
    pub name: Option<String>,
    pub piece_length: u64,
    pub announce: Vec<Vec<String>>,
    pub seeds: Option<Vec<String>>,
    pub nodes: Option<Vec<String>>,
    pub private: bool,
    pub comment: Option<String>,
    pub created_by: Option<String>,
}

pub enum TorrentAction {
    Download {
        source: TorrentSource,
        user_space: PathBuf,
    },
    Seed {
        options: SeedingOptions,
    },
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
    async fn add_torrent(
        &self,
        action: TorrentAction,
    ) -> Result<Arc<TorrentFile>, BqtiTorretingError>;

    async fn remove_torrent(&self, info_hash: InfoHash) -> Result<InfoHash, BqtiTorretingError>;
    async fn pause_torrent(&self, info_hash: InfoHash) -> Result<InfoHash, BqtiTorretingError>;
    async fn resume_torrent(&self, info_hash: InfoHash) -> Result<InfoHash, BqtiTorretingError>;
}

#[async_trait]
impl Torrenting for Bqti {
    async fn add_torrent(
        &self,
        action: TorrentAction,
    ) -> Result<Arc<TorrentFile>, BqtiTorretingError> {
        let (torrent_file, mode) = match action {
            TorrentAction::Download { source, user_space } => {
                let torrent = match source {
                    TorrentSource::TorrentFile(torrent_path) => load(&torrent_path)?,
                    TorrentSource::MagnetLink(_) => {
                        return Err(BqtiTorretingError::Unsupported("magnet links".into()));
                    }
                };

                (
                    torrent,
                    SessionMode::Download {
                        destination_dir: user_space,
                    },
                )
            }

            TorrentAction::Seed { options } => {
                let file_name = options
                    .name
                    .as_deref()
                    .or_else(|| options.path.file_name().and_then(|n| n.to_str()))
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

        let torrent_file = Arc::new(torrent_file);
        torrent_file.validate()?;

        self.torrenting_session
            .add(mode, torrent_file.clone())
            .await?;

        Ok(torrent_file)
    }

    async fn remove_torrent(&self, info_hash: InfoHash) -> Result<InfoHash, BqtiTorretingError> {
        self.torrenting_session.remove(&info_hash).await;
        Ok(info_hash)
    }

    async fn pause_torrent(&self, info_hash: InfoHash) -> Result<InfoHash, BqtiTorretingError> {
        self.torrenting_session.pause(&info_hash).await?;
        Ok(info_hash)
    }

    async fn resume_torrent(&self, info_hash: InfoHash) -> Result<InfoHash, BqtiTorretingError> {
        self.torrenting_session.resume(&info_hash).await?;
        Ok(info_hash)
    }
}

impl Bqti {
    pub fn new(
        connection_manager: Arc<ConnectionManager>,
        certificate: Arc<ActiveKeyIdentity>,
    ) -> Result<Arc<Self>> {
        let rpc_handler = Arc::new(RpcHandler::new(connection_manager.clone()));
        let kademlia = Kademlia::new(rpc_handler.clone(), certificate.clone())?;

        let pex_router = PexRouter::new(connection_manager.clone());
        let bep_router = BepRouter::new(
            certificate,
            connection_manager.clone(),
            kademlia.clone(),
            pex_router.clone(),
        )?;

        let torrenting_session = Arc::new(SessionManager::new(bep_router));

        let bqti = Arc::new(Self {
            kademlia,
            pex: pex_router,
            connection_manager,
            rpc_handler,
            torrenting_session,
        });

        Ok(bqti)
    }

    pub async fn serve_forever(
        self: &Arc<Bqti>,
        mut stream_rx: mpsc::Receiver<Packet>,
    ) -> anyhow::Result<()> {
        let mut join_set = tokio::task::JoinSet::new();
        let cancellation_token = CancellationToken::new();

        let ipc_server = IpcServer::start(self.clone())
            .await
            .context("ipc server failed to start")?;

        {
            let manager = self.connection_manager.clone();
            let cancel_tx = cancellation_token.clone();

            join_set.spawn(async move {
                manager.start_listening(cancel_tx).await;
            });
        }

        {
            let mut ipc_recv = self.torrenting_session.subscribe();
            let ipc_server = ipc_server.clone();

            join_set.spawn(async move {
                while let Ok(event) = ipc_recv.recv().await {
                    ipc_server.send_event(event).await;
                }
            });
        }

        loop {
            tokio::select! {
                Some(mut incoming_packet) = stream_rx.recv() => {
                    let bqti = Arc::clone(&self);
                    let reply = incoming_packet.take_reply();

                    join_set.spawn(async move {
                        let Some(connection) = bqti.connection_manager.get_connection(&incoming_packet.source_addr).await else {
                            warn!("received dht packet from unknown connection: {}", incoming_packet.source_addr);
                            return;
                        };

                        match incoming_packet.message {
                            Message::KeepAlive => info!("keep alive"),
                            Message::DHT(payload) => {
                                match DhtPacket::from_bytes(&payload) {
                                    Ok(packet) => {
                                        let Some(request) = bqti.rpc_handler.dispatch(packet).await else {
                                            return;
                                        };

                                        let _ = bqti.kademlia.handle_packet(request, incoming_packet.source_addr, connection).await;
                                    },
                                    Err(e) => error!("dht parse error: {}", e),
                                }
                            },
                            Message::PEX(payload)  if connection.is_inbound_authenticated().await
                                || connection.is_outbound_authenticated()  => {
                                match PexMessage::from_bytes(&payload) {
                                    Ok(pex) => {
                                        let pex_handler = bqti.pex.clone();

                                        let _ = pex_handler.handle_incoming(pex, &incoming_packet.source_addr, move |info_hash, socket| {
                                            let kademlia = bqti.kademlia.clone();

                                            async move {
                                                let _ = kademlia.announce_peer(info_hash, socket).await;
                                            }
                                        }).await;
                                    },
                                    Err(e) => error!("pex parse error: {}", e),
                                }
                            },
                            Message::Standard(payload) if connection.is_inbound_authenticated().await
                                || connection.is_outbound_authenticated()  => {
                                match StandardMessage::from_bytes(&payload) {
                                    Ok(msg) => {
                                        bqti.torrenting_session.dispatch(msg, incoming_packet.source_addr, reply).await;
                                    }
                                    Err(e) => error!("standard message parse error: {}", e),
                                }
                            },
                            _ => warn!("unauthenticated peer: {}", incoming_packet.source_addr),
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
