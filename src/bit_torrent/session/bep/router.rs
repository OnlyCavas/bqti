use std::{net::SocketAddr, sync::Arc, time::Duration};

use futures::{StreamExt, stream::FuturesUnordered};
use thiserror::Error;
use tokio::sync::{broadcast, oneshot};

use crate::{
    bit_torrent::{
        certs::CertError,
        pex::PexRouter,
        torrent::metainfo::{InfoHash, Metainfo},
    },
    certs::{ActiveKeyIdentity, KeyIdentity, PublicKey},
    dht::{
        ActiveProver, BootStrap, Kademlia, KademliaClient, Key, NodeError, TorrentDht, make_prover,
    },
    network::{
        AddressResolver, ConnectionManager, ConnectionManagerError, Message, NetworkEndpoint, Peer,
        resolve_address,
    },
    session::{
        StandardMessage, StandardMessageError,
        bep::{piece_auth, pipeline::BlockRequest, verify_piece},
        session::{PieceRequest, TorrentSession},
        state::ActiveMode,
    },
    torrent::metainfo::TorrentAddr,
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

const DHT_ANNOUNCE_INTERVAL: Duration = Duration::from_secs(30 * 60);
const DHT_DISCOVERY_INTERVAL: Duration = Duration::from_secs(1 * 60);
const PEX_FALLBACK_INTERVAL: Duration = Duration::from_secs(1 * 60);
const REBOOTSTRAP_INTERVAL: Duration = Duration::from_secs(30);

pub struct BepRouter {
    host: BepPeer,
    prover: Arc<ActiveProver>,
    connection_manager: Arc<ConnectionManager>,

    kademlia_dht: Arc<Kademlia>,
    pex_router: Arc<PexRouter>,
}

impl BepRouter {
    pub fn new(
        certificate: Arc<ActiveKeyIdentity>,
        connection_manager: Arc<ConnectionManager>,
        kademlia_dht: Arc<Kademlia>,
        pex_router: Arc<PexRouter>,
    ) -> Result<Arc<Self>, BepRouterError> {
        let bep_certificate = Arc::new(certificate.leaf("bep certificate", false)?);
        let prover = make_prover(bep_certificate.clone());

        let public_key = bep_certificate.pub_key().to_vec();
        let local_addr = connection_manager.get_local_ip()?;

        let router = Self {
            prover: Arc::new(prover),
            host: BepPeer {
                id: public_key,
                addr: local_addr,
            },
            connection_manager,
            kademlia_dht,
            pex_router,
        };

        Ok(Arc::new(router))
    }

    pub async fn disconnect(&self, addr: &SocketAddr) {
        self.connection_manager.disconnect(addr).await;
    }

    pub fn subscribe_disconnects(&self) -> broadcast::Receiver<SocketAddr> {
        self.connection_manager.subscribe_disconnects()
    }

    pub fn host(&self) -> &BepPeer {
        &self.host
    }

    pub fn prover(&self) -> Arc<ActiveProver> {
        self.prover.clone()
    }

    async fn dht_bootstrap(
        dht: &Arc<Kademlia>,
        addrs: &[TorrentAddr],
        resolver: &dyn AddressResolver,
    ) -> bool {
        let mut futs: FuturesUnordered<_> = addrs
            .iter()
            .map(|addr| {
                let addr = addr.to_string();
                let dht = dht.clone();

                async move {
                    let mut target_addr = addr;

                    // HACK It needs to be abstracted and centralized
                    if let NetworkEndpoint::I2P { socket, .. } =
                        &dht.rpc_handler.connection_manager.endpoint
                        && target_addr.contains("b32.i2p")
                    {
                        target_addr = socket.sam.get_b64_addr(&target_addr).await?;
                    }

                    match resolve_address(&target_addr, resolver) {
                        Ok(addr_solved) => {
                            let dht = dht.clone();
                            let bootstrap = BootStrap::from_socket(addr_solved);

                            Ok(dht.join_network(&bootstrap).await)
                        }
                        Err(e) => Err(anyhow::anyhow!("Resolution failed: {:?}", e)),
                    }
                }
            })
            .collect();

        while let Some(result) = futs.next().await {
            match result {
                Ok(_) => return true,
                Err(_) => warn!("failed to bootstrap for the current session"),
            }
        }

        false
    }

    pub(crate) fn start_peer_discovery(self: &Arc<Self>, session: Arc<TorrentSession>) {
        let weak_router = Arc::downgrade(self);
        let weak_session = Arc::downgrade(&session);

        tokio::spawn(async move {
            let session = match weak_session.upgrade() {
                Some(s) => s,
                None => return,
            };

            if let Some(bootstrap_nodes) = session.metadata.dht_nodes() {
                let router = match weak_router.upgrade() {
                    Some(r) => r,
                    None => return,
                };

                let resolver = router.connection_manager.endpoint.resolver();
                if !Self::dht_bootstrap(&router.kademlia_dht, bootstrap_nodes, &*resolver).await {
                    warn!("failed to bootstrap");
                }

                drop(router);
            }

            tokio::time::sleep(Duration::from_secs(5)).await;

            let info_hash = Key::from(session.metadata.info_hash());
            let is_private = session.metadata.is_private();

            let mut discovery_interval = tokio::time::interval(DHT_DISCOVERY_INTERVAL);
            let mut announce_interval = tokio::time::interval(DHT_ANNOUNCE_INTERVAL);
            let mut pex_interval = tokio::time::interval(PEX_FALLBACK_INTERVAL);
            let mut rebootstrap_interval = tokio::time::interval(REBOOTSTRAP_INTERVAL);
            let mut rebootstrap = false;

            discovery_interval.tick().await;

            loop {
                let router = match weak_router.upgrade() {
                    Some(r) => r,
                    None => return,
                };

                let session = match weak_session.upgrade() {
                    Some(s) => s,
                    None => return,
                };

                tokio::select! {
                    _ = rebootstrap_interval.tick(), if rebootstrap => {
                        let Some(bootstrap_nodes) = session.metadata.dht_nodes() else {
                            rebootstrap = false;
                            continue;
                        };

                        let resolver = router.connection_manager.endpoint.resolver();
                        rebootstrap = !Self::dht_bootstrap(&router.kademlia_dht, bootstrap_nodes, &*resolver).await;
                    },
                    _ = discovery_interval.tick() => {
                        let mut peers = Vec::new();

                        let resolver = router.connection_manager.endpoint.resolver();
                        match router.kademlia_dht.get_peers(&info_hash, &*resolver).await {
                            Ok(p) => peers.extend(p),
                            Err(_) => debug!("no peers on kademlia dht"),
                        }

                        info!("kademlia peers: {:?}", peers);

                        rebootstrap = peers.is_empty();

                        if !rebootstrap {
                            session.add_discovered_peers(peers);
                        }
                    }
                    _ = announce_interval.tick() => {
                        if !matches!(session.current_mode().await, ActiveMode::Seeding) {
                            continue;
                        }

                        match router.kademlia_dht.announce(info_hash.clone()).await {
                            Err(e) => warn!("failed to announce: {}", e),
                            Ok(_) => info!("announced to DHT"),
                        }
                    }
                    _ = pex_interval.tick(), if !is_private => {
                        let pex_peers = router.pex_router.get_peers(&info_hash).await;

                        if !pex_peers.is_empty() {
                            session.add_discovered_peers(pex_peers);
                        }
                    }
                }
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
        self.connection_manager
            .connect(&Peer::from_socket(peer))
            .await?;

        session
            .insert_pending(peer, self.host.id.clone(), true)
            .await;

        let message = StandardMessage::Handshake {
            info_hash: session.metadata.info_hash().into(),
            peer_id: self.host.id.clone(),
        };

        self.send(peer, message).await
    }

    pub async fn handle_request(
        &self,
        session: Arc<TorrentSession>,
        message: StandardMessage,
        source: SocketAddr,
        reply: Option<oneshot::Sender<Vec<u8>>>,
    ) -> Result<(), BepRouterError> {
        match &message {
            StandardMessage::Handshake { info_hash, peer_id } => {
                self.handle_handshake(session, info_hash.clone(), peer_id.clone(), source)
                    .await
            }
            StandardMessage::Bitfield(bf) => {
                self.handle_bitfield(session, bf.clone(), source).await
            }
            StandardMessage::Choke => {
                let Some(pipeline) = session.get_pipeline(source).await else {
                    return Ok(());
                };

                pipeline.on_choke().await;

                Ok(())
            }
            _ => match session.current_mode().await {
                ActiveMode::Downloading => {
                    self.handle_request_downloading(session, message, source)
                        .await
                }
                ActiveMode::Seeding => {
                    self.handle_request_seeding(session, message, source, reply)
                        .await
                }
                ActiveMode::Idle => Ok(()),
            },
        }
    }

    async fn handle_request_downloading(
        &self,
        session: Arc<TorrentSession>,
        message: StandardMessage,
        source: SocketAddr,
    ) -> Result<(), BepRouterError> {
        match message {
            StandardMessage::Unchoke => {
                let Some(pipeline) = session.get_pipeline(source).await else {
                    return Ok(());
                };

                pipeline.on_unchoke();

                let requests = pipeline.fill_requests(None, &session).await;
                let seeder_pub_key = pipeline.peer.id.clone();

                let mut in_flight: FuturesUnordered<_> = requests
                    .into_iter()
                    .map(|request| self.make_request(source, request, &seeder_pub_key))
                    .collect();

                while let Some(result) = in_flight.next().await {
                    match result {
                        Ok(Some(StandardMessage::Piece { index, data })) => {
                            let next = self
                                .process_piece(source, session.clone(), index, data)
                                .await?;

                            for request in next {
                                in_flight.push(self.make_request(source, request, &seeder_pub_key));
                            }
                        }
                        Ok(_) => {}
                        Err(e) => debug!("piece request failed: {}", e),
                    }
                }

                Ok(())
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

                return Ok(());
            }
            _ => Ok(()),
        }
    }

    async fn handle_request_seeding(
        &self,
        session: Arc<TorrentSession>,
        message: StandardMessage,
        source: SocketAddr,
        reply: Option<oneshot::Sender<Vec<u8>>>,
    ) -> Result<(), BepRouterError> {
        match message {
            StandardMessage::Interested => {
                if !matches!(session.current_mode().await, ActiveMode::Seeding) {
                    return Ok(());
                }

                let Some(pipeline) = session.get_pipeline(source).await else {
                    return Ok(());
                };

                pipeline.on_peer_interested();
                pipeline.unchoke();

                self.send(source, StandardMessage::Unchoke).await
            }
            StandardMessage::NotInterested => {
                if let Some(pipeline) = session.get_pipeline(source).await {
                    pipeline.on_peer_not_interested();
                    pipeline.choke();
                }

                self.send(source, StandardMessage::Choke).await
            }

            StandardMessage::Request { index } => {
                self.handle_piece_request(session, index, source, reply)
                    .await
            }
            StandardMessage::Have { index } => {
                let Some(pipeline) = session.get_pipeline(source).await else {
                    return Ok(());
                };

                pipeline.set_bitfield(index as usize).await;
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
        if session.get_pipeline(source).await.is_some() {
            return Ok(());
        }

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

        session.insert_pending(source, peerid_recv, true).await;
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

        if !matches!(session.current_mode().await, ActiveMode::Downloading) {
            return Ok(());
        }

        let Some(pipeline) = session.get_pipeline(source).await else {
            return Ok(());
        };

        let bitfield = session.get_bitfield().await;

        if pipeline.evaluate_interest(&bitfield).await {
            self.send(source, StandardMessage::Interested).await?;
        }

        Ok(())
    }

    fn make_request(
        &self,
        source: SocketAddr,
        request: BlockRequest,
        seeder_key: &Key,
    ) -> impl std::future::Future<Output = Result<Option<StandardMessage>, BepRouterError>> {
        let connection_manager = self.connection_manager.clone();

        async move {
            let message = Message::try_from(StandardMessage::Request {
                index: request.index,
            })?;

            let payload = connection_manager
                .request(&Peer::from_socket(source), message)
                .await?;

            let Some((data, sig)) = piece_auth::split_payload(payload) else {
                return Ok(None);
            };

            if !verify_piece(request.index, &data, &sig, seeder_key.pub_key()) {
                warn!("piece {} sig verification failed", request.index);
                return Ok(None);
            }

            let response = StandardMessage::Piece {
                index: request.index,
                data,
            };

            Ok(Some(response))
        }
    }

    async fn process_piece(
        &self,
        source: SocketAddr,
        session: Arc<TorrentSession>,
        index: u32,
        data: Vec<u8>,
    ) -> Result<Vec<BlockRequest>, BepRouterError> {
        let pipeline = match session.get_pipeline(source).await {
            Some(p) => p,
            None => return Ok(vec![]),
        };

        if !session.add_piece(index).await {
            return Ok(pipeline.fill_requests(None, &session).await);
        }

        pipeline.clear_piece(index).await;
        session.send_downloaded_piece(index, data).await;

        let session_bitfield = session.get_bitfield().await;

        info!(
            "piece {} complete, have {}/{}",
            index,
            session_bitfield.count(),
            session_bitfield.piece_count
        );

        session.broadcast_have(index).await;

        if session_bitfield.is_complete() {
            return Ok(vec![]);
        }

        Ok(pipeline.fill_requests(Some(index), &session).await)
    }

    async fn handle_piece_request(
        &self,
        session: Arc<TorrentSession>,
        index: u32,
        source: SocketAddr,
        reply: Option<oneshot::Sender<Vec<u8>>>,
    ) -> Result<(), BepRouterError> {
        let Some(pipeline) = session.get_pipeline(source).await else {
            return Ok(());
        };

        if pipeline.we_are_choking() {
            return Ok(());
        }

        let session_bitfield = session.get_bitfield().await;

        if !session_bitfield.have(index as usize) {
            return Ok(());
        }

        let (respond_tx, respond_rx) = oneshot::channel();

        session
            .send_piece_request(PieceRequest {
                index,
                respond: respond_tx,
            })
            .await;

        tokio::spawn(async move {
            let Ok(data) = respond_rx.await else { return };

            if let Some(tx) = reply {
                let _ = tx.send(data);
            }
        });

        Ok(())
    }
}
