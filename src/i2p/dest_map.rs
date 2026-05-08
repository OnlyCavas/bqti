use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

#[derive(Default)]
struct DestMapInner {
    dest_to_addr: HashMap<String, SocketAddr>,
    addr_to_dest: HashMap<SocketAddr, String>,
}

pub struct DestMap {
    inner: Mutex<DestMapInner>,
    next_ip: AtomicU32,
}

impl DestMap {
    pub fn new() -> Self {
        Self {
            next_ip: AtomicU32::new(1),
            inner: Mutex::new(DestMapInner::default()),
        }
    }

    pub fn resolve(&self, addr: &SocketAddr) -> Option<String> {
        self.inner.lock().unwrap().addr_to_dest.get(addr).cloned()
    }

    pub fn get_or_insert(&self, dest: &str) -> SocketAddr {
        let mut inner = self.inner.lock().unwrap();

        if let Some(&addr) = inner.dest_to_addr.get(dest) {
            return addr;
        }

        let prefix_match = inner
            .dest_to_addr
            .iter()
            .find(|(k, _)| k.starts_with(dest) || dest.starts_with(k.as_str()))
            .map(|(_, &addr)| addr);

        if let Some(addr) = prefix_match {
            inner.dest_to_addr.insert(dest.to_string(), addr);

            if let Some(existing) = inner.addr_to_dest.get(&addr) {
                if dest.len() > existing.len() {
                    inner.addr_to_dest.insert(addr, dest.to_string());
                }
            }

            return addr;
        }

        let ip = self.next_ip.fetch_add(1, Ordering::Relaxed);
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(0x0A000000 + ip), 4242));

        inner.dest_to_addr.insert(dest.to_string(), addr);
        inner.addr_to_dest.insert(addr, dest.to_string());

        addr
    }
}
