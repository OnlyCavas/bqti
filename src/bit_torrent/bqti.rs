use std::sync::Arc;

use anyhow::Result;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

use crate::{
    dht::{BootStrap, DhtPacket, Kademlia, Node, RpcHandler},
    network::{ConnectionManager, Message, Packet},
};

pub struct Bqti {
    pub kademlia: Arc<Kademlia>,                // dht
    connection_manager: Arc<ConnectionManager>, // single channel message flow
    rpc_handler: Arc<RpcHandler>,               // rpc call abstraction ontop of connection manager
}

impl Bqti {
    pub fn new(addr: &str, connection_manager: Arc<ConnectionManager>) -> Result<Self> {
        let rpc_handler = Arc::new(RpcHandler::new(connection_manager.clone()));
        let kademlia = Kademlia::new(addr, rpc_handler.clone())?;

        let bqti = Self {
            kademlia: Arc::new(kademlia),
            connection_manager,
            rpc_handler,
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
                    let rpc_handler = self.rpc_handler.clone();

                    join_set.spawn(async move {
                      match message {
                        Message::KeepAlive => info!("keep alive"),
                        Message::DHT(payload) => {
                            match DhtPacket::from_bytes(&payload) {
                                Ok(packet) => {
                                    let Some(request) = rpc_handler.dispatch(packet).await else {
                                        return;
                                    };

                                    let _ = kademlia.handle_request(request, source_addr).await;
                                },
                                Err(e) => error!("dht parse error: {}", e),
                            }
                        },
                        Message::PEX(payload) => info!("pex: {}", hex::encode(payload)),
                        Message::Standard(payload) => info!("bit: {}", hex::encode(payload)),
                    }

                    });
                },
                 Ok(Some(line)) = lines.next_line() => {
                    let kademlia = self.kademlia.clone();

                    match line.trim() {
                        "o" => {
                            info!("pressed o");

                            join_set.spawn(async move {
                                let node = match Node::new("127.0.0.1:9001") {
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
                                        Ok(_) => info!("it bootstrap!"),
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
