use std::{
    io,
    net::SocketAddr,
    sync::{Arc, Mutex},
    task::Poll,
};

use futures::task::AtomicWaker;
use quinn::{AsyncUdpSocket, UdpPoller, udp::Transmit};
use tokio::sync::mpsc::{self, error::TryRecvError};
use tokio_util::sync::CancellationToken;

use super::{dest_map::DestMap, session::SamSession};

const UDP_PACKET_SIZE: usize = 65536;
const UDP_CHANNEL_SIZE: usize = 512;

const SAM_UDP_ADDR: &str = "127.0.0.1:7655";

type Datagram = (Vec<u8>, SocketAddr);

impl std::fmt::Debug for I2pDatagramSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("I2pDatagramSocket")
            .field("session_id", &self.sam.session_id)
            .field("local_addr", &self.local_addr)
            .finish()
    }
}

pub struct I2pDatagramSocket {
    pub sam: Arc<SamSession>,
    pub dest_map: Arc<DestMap>,
    local_addr: SocketAddr,
    inbound_rx: Mutex<mpsc::Receiver<Datagram>>,
    waker: Arc<AtomicWaker>,
    _cancel: tokio_util::sync::DropGuard,
}

impl I2pDatagramSocket {
    pub fn register_dest(&self, b64_dest: &str) -> SocketAddr {
        self.dest_map.get_or_insert(b64_dest)
    }

    pub fn new(sam: Arc<SamSession>, dest_map: Arc<DestMap>) -> Self {
        let waker = Arc::new(AtomicWaker::new());
        let (tx, rx) = mpsc::channel(UDP_CHANNEL_SIZE);
        let token = CancellationToken::new();

        let local_addr = dest_map.get_or_insert(&sam.destination);

        tokio::spawn(recv_loop(
            sam.clone(),
            dest_map.clone(),
            tx,
            waker.clone(),
            token.clone(),
        ));

        Self {
            sam,
            dest_map,
            local_addr,
            inbound_rx: std::sync::Mutex::new(rx),
            waker,
            _cancel: token.drop_guard(),
        }
    }
}

async fn recv_loop(
    sam: Arc<SamSession>,
    dest_map: Arc<DestMap>,
    tx: mpsc::Sender<(Vec<u8>, SocketAddr)>,
    waker: Arc<AtomicWaker>,
    token: CancellationToken,
) {
    let mut buf = vec![0u8; UDP_PACKET_SIZE];

    loop {
        tokio::select! {
            _ = token.cancelled() => break,

            result = sam.udp.recv(&mut buf) => {
                let n = match result {
                    Ok(n) => n,
                    Err(e) => {
                        error!("SAM datagram recv error: {e}");
                        break;
                    }
                };

                let data = &buf[..n];

                let Some(nl) = data.iter().position(|&b| b == b'\n') else {
                    error!("SAM datagram missing newline separator");
                    continue;
                };

                let source = match std::str::from_utf8(&data[..nl]) {
                    Ok(s) => s,
                    Err(_) => {
                        error!("SAM datagram non-UTF8 source destination");
                        continue;
                    }
                };

                let payload = data[nl + 1..].to_vec();
                let src_addr = dest_map.get_or_insert(&source);

                if tx.send((payload, src_addr)).await.is_err() {
                    break;
                }

                waker.wake();
            }
        }
    }
}

#[derive(Debug)]
struct I2pPoller;

impl UdpPoller for I2pPoller {
    fn poll_writable(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncUdpSocket for I2pDatagramSocket {
    fn create_io_poller(self: Arc<Self>) -> std::pin::Pin<Box<dyn UdpPoller>> {
        Box::pin(I2pPoller)
    }

    fn try_send(&self, transmit: &Transmit) -> io::Result<()> {
        let sam_addr: SocketAddr = SAM_UDP_ADDR
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::AddrNotAvailable, "invalid sam address"))?;

        let session_id = self.sam.session_id.clone();

        let Some(destination) = self.dest_map.resolve(&transmit.destination) else {
            return Ok(());
        };

        let udp_header = format!("3.0 {} {}\n", session_id, destination);
        let mut packet = udp_header.into_bytes();
        packet.extend_from_slice(transmit.contents.as_ref());

        match self.sam.udp.try_send_to(&packet, sam_addr) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                Err(io::Error::new(io::ErrorKind::WouldBlock, e))
            }
            Err(e) => Err(e),
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local_addr)
    }

    fn poll_recv(
        &self,
        cx: &mut std::task::Context,
        bufs: &mut [io::IoSliceMut<'_>],
        meta: &mut [quinn::udp::RecvMeta],
    ) -> Poll<io::Result<usize>> {
        self.waker.register(cx.waker());

        let mut rx = self.inbound_rx.lock().unwrap();
        let mut n = 0;

        for (buf, meta_slot) in bufs.iter_mut().zip(meta.iter_mut()) {
            match rx.try_recv() {
                Ok((packet, src_addr)) => {
                    let len = packet.len().min(buf.len());
                    buf[..len].copy_from_slice(&packet[..len]);

                    *meta_slot = quinn::udp::RecvMeta {
                        addr: src_addr,
                        len,
                        stride: len,
                        ecn: None,
                        dst_ip: Some(self.local_addr.ip()),
                    };

                    n += 1;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "inbound channel closed",
                    )));
                }
            }
        }

        if n > 0 {
            Poll::Ready(Ok(n))
        } else {
            Poll::Pending
        }
    }
}
