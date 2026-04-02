use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

use crate::{
    bit_torrent::{
        certs::{KeyIdentity, PublicKey},
        pex::{PexMessage, PexRouter},
        torrent::metainfo::{Integrity, TorrentFile},
    },
    dht::{BootStrap, DhtPacket, Kademlia, KademliaClient, Node, RpcHandler, TorrentDht},
    load,
    network::{ConnectionManager, Message, Packet},
    session::{BepRouter, SessionManager, SessionMode, StandardMessage, TorrentSessionError},
};

#[derive(Debug, Error)]
pub enum BqtiTorretingError {
    #[error(transparent)]
    TorrentSessionError(#[from] TorrentSessionError),
}

#[async_trait]
pub trait Torreting {
    async fn append(
        &self,
        mode: SessionMode,
        torrent: TorrentFile,
    ) -> Result<(), BqtiTorretingError>;
}

pub struct Bqti {
    connection_manager: Arc<ConnectionManager>,

    pub kademlia: Arc<Kademlia>,
    rpc_handler: Arc<RpcHandler>,

    pex: Arc<PexRouter>,

    manager: Arc<SessionManager>,
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
            manager,
        };

        Ok(bqti)
    }

    pub async fn serve_forever(&self, mut stream_rx: mpsc::Receiver<Packet>) -> Result<()> {
        let mut join_set = tokio::task::JoinSet::new();
        let cancellation_token = CancellationToken::new();

        let stdin = BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();

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
                    let torrent_manager = self.manager.clone();

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
                 Ok(Some(line)) = lines.next_line() => {
                    let kademlia = self.kademlia.clone();

                    match line.trim() {
                        "o" => {
                            info!("pressed o");

                            join_set.spawn(async move {
                                let node = match Node::random("127.0.0.1:9001") {
                                    Ok(n) => n,
                                    Err(e) => { error!("invalid node addr: {}", e); return; }
                                };

                                match kademlia.ping(&node).await {
                                    Ok(r) => info!("ping response: {:?}", r),
                                    Err(e) => error!("ping failed: {}", e),
                                }
                            });
                        },
                        "j" => {
                            info!("pressed j");

                            join_set.spawn(async move {
                                let node = match BootStrap::new("127.0.0.1:9002") {
                                    Ok(n) => n,
                                    Err(e) => { error!("invalid node addr: {}", e); return; }
                                };

                                match kademlia
                                    .join_network(&node)
                                    .await {
                                        Ok(_) => info!("connected to the network"),
                                        Err(_) => warn!("failed to boostrap"),
                                    }
                            });
                        },
                        "c" => {
                            info!("pressed c");

                            join_set.spawn(async move {
                                let route_table = kademlia.route_table.read().await;
                                info!("{:?}", route_table);
                            });
                        },
                        "d" => {
                            let Ok(torrent_file) = load("resources/yes.torrent") else {
                                panic!("failed to load file");
                            };

                            let Ok(_) = torrent_file.validate() else {
                                panic!("it's invalid");
                            };

                            self.append(SessionMode::Download { target_dir: PathBuf::from(".") }, torrent_file.clone()).await?;
                        },
                        "s" => {
                            let Ok(torrent_file) = load("resources/yes.torrent") else {
                                panic!("failed to load file");
                            };

                            let Ok(_) = torrent_file.validate() else {
                                panic!("it's invalid");
                            };

                            self.append(SessionMode::Seed { source_dir: PathBuf::from("resources") }, torrent_file.clone()).await?;
                       },
                        "q" => break,
                        _ => {}
                    }
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
}

#[async_trait]
impl Torreting for Bqti {
    async fn append(
        &self,
        mode: SessionMode,
        torrent: TorrentFile,
    ) -> Result<(), BqtiTorretingError> {
        self.manager.add(mode, &torrent).await;

        Ok(())
    }
}
