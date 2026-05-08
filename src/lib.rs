#[macro_use]
extern crate tracing;

mod bit_torrent;
mod error;

pub mod cli;
pub mod daemon;
pub mod standalone;
pub mod utils;

pub mod i2p;
pub mod ipc;

pub use bit_torrent::*;
pub use error::BQTIError;

use crate::{
    bit_torrent::network::{NetworkEndpoint, QuicEndpointBuilder},
    i2p::I2pEndpointBuilder,
};

pub enum EndpointBuilder {
    Quic(QuicEndpointBuilder),
    I2p(I2pEndpointBuilder),
}

impl EndpointBuilder {
    pub async fn build(self) -> anyhow::Result<NetworkEndpoint> {
        match self {
            Self::Quic(b) => {
                let endpoint = b.build()?;

                Ok(NetworkEndpoint::Standard(endpoint))
            }
            Self::I2p(b) => {
                let endpoint = b.build().await?;

                Ok(NetworkEndpoint::I2P {
                    endpoint: endpoint.0,
                    socket: endpoint.1,
                })
            }
        }
    }

    pub fn dangerous_no_cert_verify(self) -> Self {
        match self {
            Self::Quic(b) => Self::Quic(b.dangerous_no_cert_verify()),
            Self::I2p(b) => Self::I2p(b.dangerous_no_cert_verify()),
        }
    }
}
