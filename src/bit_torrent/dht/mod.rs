mod auth;
mod k_bucket;
mod kademlia;
mod key_id;
mod manifest;
mod message;
mod node;
mod route_table;
mod rpc;
mod store;

pub type RequestId = u64;

use std::{collections::HashSet, net::SocketAddr};

use async_trait::async_trait;
pub use kademlia::{Kademlia, KademliaClient, KademliaError, KademliaServer};
pub use key_id::{KEY_ID_LENGTH, Key, OrdDistance};
pub use manifest::{Manifest, ManifestError};
pub use message::{DhtMessageError, DhtPacket, DhtResponse, KademliaData, RpcRequest, RpcResponse};
pub use node::{BootStrap, Node, NodeError};
pub use rpc::{RpcError, RpcHandler};

use crate::{
    bit_torrent::torrent::metainfo::InfoHash,
    network::{AddressResolver, NetworkEndpoint},
};

#[async_trait]
pub trait TorrentDht {
    async fn announce(&self, info_hash: Key) -> Result<(), KademliaError>;
    async fn announce_peer(&self, info_hash: Key, addr: SocketAddr) -> Result<(), KademliaError>;

    async fn get_peers(
        &self,
        info_hash: &Key,
        resolver: &dyn AddressResolver,
    ) -> Result<Vec<SocketAddr>, KademliaError>;
}

#[async_trait]
impl TorrentDht for Kademlia {
    async fn announce(&self, info_hash: Key) -> Result<(), KademliaError> {
        let host_addr = {
            let route_table = self.route_table.read().await;
            route_table.host.addr
        };

        let peers_data = if let NetworkEndpoint::I2P { ref socket, .. } =
            self.rpc_handler.connection_manager.endpoint
        {
            KademliaData::I2Peers(HashSet::from([socket.sam.destination.clone()]))
        } else {
            KademliaData::Peers(HashSet::from([host_addr]))
        };

        self.store(info_hash, peers_data).await
    }

    async fn announce_peer(&self, info_hash: Key, addr: SocketAddr) -> Result<(), KademliaError> {
        self.store(info_hash, KademliaData::Peers(HashSet::from([addr])))
            .await
    }

    async fn get_peers(
        &self,
        info_hash: &Key,
        resolver: &dyn AddressResolver,
    ) -> Result<Vec<SocketAddr>, KademliaError> {
        let found_data = self.find_value(&info_hash).await?;

        match found_data {
            Some(KademliaData::Value(..)) => Err(KademliaError::NoValue()),
            Some(KademliaData::I2Peers(peers)) => Ok(peers
                .iter()
                .filter_map(|b64| resolver.resolve(b64).ok())
                .collect()),
            Some(KademliaData::Peers(peers)) => Ok(peers.into_iter().collect()),
            None => Err(KademliaError::NoValue()),
        }
    }
}

impl From<&InfoHash> for Key {
    fn from(value: &InfoHash) -> Self {
        Self::new(value.as_ref())
    }
}

pub use auth::{ActiveProver, AuthError, AuthManager, ProveChallenge, make_prover};
