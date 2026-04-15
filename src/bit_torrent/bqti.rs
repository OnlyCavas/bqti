use std::{convert::Infallible, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc::{self, Receiver};
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::{
        certs::{KeyIdentity, PublicKey},
        pex::{PexMessage, PexRouter},
        torrent::metainfo::{Integrity, Metainfo, TorrentError},
    },
    dht::{DhtPacket, Kademlia, RpcHandler, TorrentDht},
    ipc::server::IpcCommand,
    load,
    network::{ConnectionManager, Message, Packet},
    session::{BepRouter, SessionManager, SessionMode, StandardMessage, TorrentSessionError},
};

#[derive(Debug, Error)]
pub enum BqtiTorretingError {
    #[error(transparent)]
    TorrentSessionError(#[from] TorrentSessionError),

    #[error(transparent)]
    TorrentError(#[from] TorrentError),

    #[error("failed to load .torrent file")]
    FailedToLoad(),

    #[error("unsupported feature: {0}")]
    Unsupported(String),
}

pub enum TorrentSource {
    Magnet(String),
    File(PathBuf),
}

impl FromStr for TorrentSource {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("magnet:") {
            Ok(Self::Magnet(s.to_string()))
        } else {
            Ok(Self::File(PathBuf::from(s)))
        }
    }
}

#[async_trait]
pub trait Torrenting {
    // TODO remove torrents
    // TODO get torrent status, an stream of events of the download or seeding state
    async fn add_torrent(
        &self,
        mode: SessionMode,
        source: TorrentSource,
    ) -> Result<String, BqtiTorretingError>;
}

pub struct Bqti {
    connection_manager: Arc<ConnectionManager>,
    kademlia: Arc<Kademlia>,
    rpc_handler: Arc<RpcHandler>,
    pex: Arc<PexRouter>,
    torrenting_session: Arc<SessionManager>,
}

impl Bqti {
    pub fn new(
        connection_manager: Arc<ConnectionManager>,
        certificate: KeyIdentity,
    ) -> Result<Self> {
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

        let bqti = Self {
            kademlia,
            pex: pex_router,
            connection_manager,
            rpc_handler,
            torrenting_session: manager,
        };

        Ok(bqti)
    }

    pub async fn serve_forever(
        &mut self,
        mut stream_rx: mpsc::Receiver<Packet>,
        mut ipc_rx: Receiver<IpcCommand>,
    ) -> Result<()> {
        let mut join_set = tokio::task::JoinSet::new();
        let cancellation_token = CancellationToken::new();

        let manager = self.connection_manager.clone();
        let cancel_tx = cancellation_token.clone();

        let listener = tokio::spawn(async move {
            manager.start_listening(cancel_tx).await;
        });

        loop {
            tokio::select! {
                Some(Packet(message, source_addr)) = stream_rx.recv() => {
                    let kademlia = self.kademlia.clone();
                    let pex_handler = self.pex.clone();
                    let rpc_handler = self.rpc_handler.clone();
                    let torrent_manager = self.torrenting_session.clone();

                    join_set.spawn(async move {
                      match message {
                        Message::KeepAlive => info!("keep alive"),
                        Message::DHT(payload) => {
                            match DhtPacket::from_bytes(&payload) {
                                Ok(packet) => {
                                    let Some(request) = rpc_handler.dispatch(packet).await else {
                                        return;
                                    };

                                    let _ = kademlia.handle_packet(request, source_addr).await;
                                },
                                Err(e) => error!("dht parse error: {}", e),
                            }
                        },
                        Message::PEX(payload) => {
                            match PexMessage::from_bytes(&payload) {
                                Ok(pex) => {
                                    let _ = pex_handler.handle_incoming(pex, &source_addr, move |info_hash, socket| {
                                        let kademlia = kademlia.clone();

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
                                    torrent_manager.dispatch(msg, source_addr).await;
                                }
                                Err(e) => error!("standard message parse error: {}", e),
                            }
                        }
                      }
                    });
                },

                Some(cmd) = ipc_rx.recv() => {
                    self.handle_ipc(cmd).await;
                }

                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }

        cancellation_token.cancel();
        self.connection_manager.shutdown().await;
        join_set.shutdown().await;
        listener.abort();

        Ok(())
    }

    async fn handle_ipc(&self, cmd: IpcCommand) {
        match cmd {
            IpcCommand::AddTorrent { mode, reply } => {
                let source = TorrentSource::File(mode.dir().clone());

                let result = self
                    .add_torrent(mode, source)
                    .await
                    .map_err(|e| e.to_string());

                let _ = reply.send(result);
            }
        }
    }
}

#[async_trait]
impl Torrenting for Bqti {
    async fn add_torrent(
        &self,
        mode: SessionMode,
        source: TorrentSource,
    ) -> Result<String, BqtiTorretingError> {
        let torrent = match source {
            TorrentSource::Magnet(_uri) => {
                Err(BqtiTorretingError::Unsupported("magnet links".into()))
            }
            TorrentSource::File(path) => {
                load(&path).map_err(|_| BqtiTorretingError::FailedToLoad())
            }
        }?;

        torrent.validate()?;
        let info_hash = torrent.info_hash().to_string();

        self.torrenting_session.add(mode, &torrent).await;

        Ok(info_hash)
    }
}
