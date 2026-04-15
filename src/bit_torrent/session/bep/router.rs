use std::{net::SocketAddr, sync::Arc, time::Duration};

use thiserror::Error;
use tokio::sync::oneshot;

use crate::{
    bit_torrent::{
        certs::CertError,
        pex::PexRouter,
        torrent::metainfo::{InfoHash, Metainfo},
    },
    dht::{BootStrap, Kademlia, KademliaClient, Key, NodeError, TorrentDht},
    network::{ConnectionManager, ConnectionManagerError, Message, Peer},
    session::{
        StandardMessage, StandardMessageError,
        session::{PieceRequest, TorrentSession},
    },
};

pub struct BepPeer {
    id: Vec<u8>,
    pub addr: SocketAddr,
}

#[derive(Debug, Error)]
pub enum BepRouterError {
    #[error(transparent)]
    NodeError(#[from] NodeError),

    #[error(transparent)]
    CertErr(#[from] CertError),

    #[error(transparent)]
    StandardMessageError(#[from] StandardMessageError),

    #[error("failed to handle request")]
    HandleFailed(),

    #[error(transparent)]
    ConnectionManagerError(#[from] ConnectionManagerError),

    #[error("no active pipeline found for that address")]
    NonActivePipeline(),
}

const PEER_DISCOVERY_INTERVAL: Duration = Duration::from_hours(30);

pub struct BepRouter {
    host: BepPeer,
    connection_manager: Arc<ConnectionManager>,

    kademlia_dht: Arc<Kademlia>,
    pex_router: Arc<PexRouter>,
}

impl BepRouter {
    pub fn new(
        pub_key: &[u8],
        connection_manager: Arc<ConnectionManager>,
        kademlia_dht: Arc<Kademlia>,
        pex_router: Arc<PexRouter>,
    ) -> Result<Arc<Self>, BepRouterError> {
        let local_addr = connection_manager.get_local_ip()?;

        let router = Self {
            host: BepPeer {
                id: pub_key.to_vec(),
                addr: local_addr,
            },
            connection_manager,
            kademlia_dht,
            pex_router,
        };

        Ok(Arc::new(router))
    }

    pub fn host(&self) -> &BepPeer {
        &self.host
    }

    pub(crate) fn start_peer_discovery(self: &Arc<Self>, session: Arc<TorrentSession>) {
        let weak_prt = Arc::downgrade(self);
        let torrent_session = session.clone();

        tokio::spawn(async move {
            let info_hash = &Key::from(torrent_session.metadata.info_hash());
            let mut interval = tokio::time::interval(PEER_DISCOVERY_INTERVAL);

            if let Some(bootstrap_addrs) = torrent_session.metadata.dht_nodes() {
                let Some(router) = weak_prt.upgrade() else {
                    return;
                };

                for addr in bootstrap_addrs {
                    let bootstrap = BootStrap::from_socket(*addr);

                    if let Ok(_) = router.kademlia_dht.join_network(&bootstrap).await {
                        break;
                    }
                }
            }

            loop {
                interval.tick().await;

                let router = match weak_prt.upgrade() {
                    Some(s) => s,
                    None => return,
                };

                let mut possible_peers: Vec<SocketAddr> = Vec::new();

                match router.kademlia_dht.get_peers(info_hash).await {
                    Ok(peers) => possible_peers.extend(peers),
                    Err(_) => {
                        debug!("couldn't find any peers on kademlia dht table");
                    }
                }

                let pex_peers = router.pex_router.get_peers(info_hash).await;
                possible_peers.extend(pex_peers);

                if !possible_peers.is_empty() {
                    torrent_session.add_discovered_peers(possible_peers);
                }

                // NOTE should it announce for download?
                match router.kademlia_dht.announce(info_hash.clone()).await {
                    Err(e) => warn!("failed to announce: {}", e.to_string()),
                    _ => info!("announce"),
                };
            }
        });
    }

    pub async fn send(&self, peer: SocketAddr, bep: StandardMessage) -> Result<(), BepRouterError> {
        let message = Message::try_from(bep)?;

        self.connection_manager
            .send(&Peer::from_socket(peer), message)
            .await?;

        Ok(())
    }

    pub async fn handshake(
        &self,
        session: Arc<TorrentSession>,
        peer: SocketAddr,
    ) -> Result<(), BepRouterError> {
        session
            .insert_pending(peer, self.host.id.clone(), true)
            .await;

        if peer == self.host.addr {
            return Ok(());
        }

        let message = StandardMessage::Handshake {
            info_hash: session.metadata.info_hash().into(),
            peer_id: self.host.id.clone(),
        };

        self.send(peer, message).await?;

        Ok(())
    }

    pub async fn handle_request(
        &self,
        session: Arc<TorrentSession>,
        message: StandardMessage,
        source: SocketAddr,
    ) -> Result<(), BepRouterError> {
        match message {
            StandardMessage::Handshake {
                info_hash: infohash_recv,
                peer_id: peerid_recv,
            } => {
                self.handle_handshake(session, infohash_recv, peerid_recv, source)
                    .await
            }
            StandardMessage::Bitfield(bitfield_recv) => {
                self.handle_bitfield(session, bitfield_recv, source).await
            }
            StandardMessage::Interested => {
                let Some(pipeline) = session.get_pipeline(source).await else {
                    return Err(BepRouterError::NonActivePipeline());
                };

                pipeline.on_peer_interested();
                self.send(source, StandardMessage::Unchoke).await
            }
            StandardMessage::Unchoke => {
                let Some(pipeline) = session.get_pipeline(source).await else {
                    return Err(BepRouterError::NonActivePipeline());
                };

                pipeline.on_unchoke();

                let requests = pipeline
                    .fill_requests(None, &session, |i| session.piece_size(i))
                    .await;

                for request in requests {
                    self.send(
                        source,
                        StandardMessage::Request {
                            index: request.index,
                            begin: request.begin,
                            length: request.length,
                        },
                    )
                    .await?;
                }

                Ok(())
            }
            StandardMessage::Request {
                index,
                begin,
                length,
            } => {
                self.handle_piece_request(session, index, begin, length, source)
                    .await
            }

            StandardMessage::Piece { index, begin, data } => {
                self.handle_piece(session, index, begin, data, source).await
            }
            StandardMessage::Have { index } => {
                let Some(pipeline) = session.get_pipeline(source).await else {
                    return Err(BepRouterError::NonActivePipeline());
                };

                pipeline.set_bitfield(index as usize).await;
                let session_bitfield = session.get_bitfield().await;

                if pipeline.evaluate_interest(&session_bitfield).await {
                    self.send(source, StandardMessage::Interested).await?;
                }

                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn handle_handshake(
        &self,
        session: Arc<TorrentSession>,
        infohash_recv: Vec<u8>,
        peerid_recv: Vec<u8>,
        source: SocketAddr,
    ) -> Result<(), BepRouterError> {
        session.check_already_active(source).await;

        let infohash_recv =
            InfoHash::try_from(infohash_recv).map_err(|_| BepRouterError::HandleFailed())?;

        if infohash_recv != *session.metadata.info_hash() {
            return Err(BepRouterError::HandleFailed());
        }

        if !session.is_pending_outbound(&source).await {
            session.insert_pending(source, peerid_recv, false).await;

            self.send(
                source,
                StandardMessage::Handshake {
                    info_hash: session.metadata.info_hash().into(),
                    peer_id: self.host.id.clone(),
                },
            )
            .await?;

            return Ok(());
        }

        let bitfield_bits = session.get_bitfield().await.as_bytes().to_vec();

        self.send(source, StandardMessage::Bitfield(bitfield_bits))
            .await?;

        Ok(())
    }

    async fn handle_bitfield(
        &self,
        session: Arc<TorrentSession>,
        bitfield_recv: Vec<u8>,
        source: SocketAddr,
    ) -> Result<(), BepRouterError> {
        let self_init = session.is_pending_outbound(&source).await;

        let Some(peer_id) = session.get_pending_peer(source).await else {
            warn!("bitfield from unknown peer {}, dropping", source);
            return Ok(());
        };

        session
            .insert_pipeline(source, peer_id, bitfield_recv)
            .await;

        if !self_init {
            let bitfield = session.get_bitfield().await;
            let bits = bitfield.as_bytes().to_vec();

            self.send(source, StandardMessage::Bitfield(bits)).await?;
        }

        let session_bitfield = session.get_bitfield().await;
        let Some(pipeline) = session.get_pipeline(source).await else {
            return Err(BepRouterError::HandleFailed());
        };

        if pipeline.evaluate_interest(&session_bitfield).await {
            self.send(source, StandardMessage::Interested).await?;
        }

        Ok(())
    }

    async fn handle_piece(
        &self,
        session: Arc<TorrentSession>,
        index: u32,
        begin: u32,
        data: Vec<u8>,
        source: SocketAddr,
    ) -> Result<(), BepRouterError> {
        let session_bitfield = session.get_bitfield().await;

        if session_bitfield.is_complete() {
            return Ok(());
        }

        let pipeline = match session.get_pipeline(source).await {
            Some(p) => p,
            None => return Ok(()),
        };

        let complete = session.add_block(index, begin, data).await;

        if let Some(piece_data) = complete {
            pipeline.clear_piece(index).await;
            session.send_downloaded_piece(index, piece_data).await;

            debug!(
                "piece {} complete, have {}/{}",
                index,
                session_bitfield.count(),
                session_bitfield.piece_count
            );

            session.broadcast_have(index).await;

            if session_bitfield.is_complete() {
                debug!("torrent complete, disconnecting all peers");
                session.shutdown_peers().await;
            }

            pipeline.clear_piece(index).await;
        }

        let requests = pipeline
            .fill_requests(Some((index, begin)), &session, |i| session.piece_size(i))
            .await;

        for request in requests {
            self.send(
                source,
                StandardMessage::Request {
                    index: request.index,
                    begin: request.begin,
                    length: request.length,
                },
            )
            .await?;
        }

        Ok(())
    }

    async fn handle_piece_request(
        &self,
        session: Arc<TorrentSession>,
        index: u32,
        begin: u32,
        length: u32,
        source: SocketAddr,
    ) -> Result<(), BepRouterError> {
        let session_bitfield = session.get_bitfield().await;

        if !session_bitfield.have(index as usize) {
            warn!(
                "peer {} requested piece {}, but i don't provide it",
                source, index
            );

            return Ok(());
        }

        let (respond_tx, respond_rx) = oneshot::channel();

        session
            .send_piece_request(PieceRequest {
                index,
                begin,
                length,
                respond: respond_tx,
            })
            .await;

        let data = respond_rx
            .await
            .map_err(|_| BepRouterError::HandleFailed())?;

        self.send(source, StandardMessage::Piece { index, begin, data })
            .await?;

        Ok(())
    }
}
