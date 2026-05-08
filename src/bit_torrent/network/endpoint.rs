use std::{io, net::SocketAddr, sync::Arc};

use quinn::Endpoint;

use crate::{
    i2p::I2pDatagramSocket,
    network::resolver::{AddressResolver, I2PResolver, IPResolver},
};

#[derive(Clone)]
pub enum NetworkEndpoint {
    Standard(Endpoint),
    I2P {
        endpoint: Endpoint,
        socket: Arc<I2pDatagramSocket>,
    },
}

impl NetworkEndpoint {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match self {
            NetworkEndpoint::Standard(endpoint) | NetworkEndpoint::I2P { endpoint, .. } => {
                endpoint.local_addr()
            }
        }
    }

    pub fn resolver(&self) -> Box<dyn AddressResolver> {
        match self {
            NetworkEndpoint::Standard(..) => Box::new(IPResolver),
            NetworkEndpoint::I2P { socket, .. } => {
                Box::new(I2PResolver::new(socket.dest_map.clone()))
            }
        }
    }

    pub(crate) fn inner(&self) -> &quinn::Endpoint {
        match self {
            NetworkEndpoint::Standard(endpoint) | NetworkEndpoint::I2P { endpoint, .. } => {
                &endpoint
            }
        }
    }
}
