use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;

use crate::i2p::DestMap;

pub trait AddressResolver: Send + Sync {
    fn resolve(&self, addr: &str) -> anyhow::Result<SocketAddr>;
}

#[derive(Default)]
pub struct IPResolver;

impl AddressResolver for IPResolver {
    fn resolve(&self, addr: &str) -> anyhow::Result<SocketAddr> {
        addr.parse::<SocketAddr>()
            .context("failed to parse IP address")
    }
}

pub struct I2PResolver {
    dest_map: Arc<DestMap>,
}

impl I2PResolver {
    pub fn new(dest_map: Arc<DestMap>) -> Self {
        Self { dest_map }
    }
}

impl AddressResolver for I2PResolver {
    fn resolve(&self, addr: &str) -> anyhow::Result<SocketAddr> {
        if let Ok(socket_addr) = addr.parse::<SocketAddr>() {
            return Ok(socket_addr);
        }

        if addr.ends_with(".b32.i2p") || addr.len() > 200 {
            return Ok(self.dest_map.get_or_insert(addr));
        }

        Err(anyhow::anyhow!("Invalid address format: {}", addr))
    }
}

pub fn resolve_address(addr: &str, resolver: &dyn AddressResolver) -> anyhow::Result<SocketAddr> {
    anyhow::ensure!(!addr.is_empty(), "address can't be empty");
    resolver.resolve(addr)
}
