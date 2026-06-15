use base64::{Engine, engine::general_purpose};
use data_encoding::BASE32_NOPAD;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use thiserror::Error;
use tokio::{
    io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        TcpStream, UdpSocket,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::Mutex,
};

#[derive(Debug, Error)]
pub enum SamError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("sam bridge error: {0}")]
    Bridge(String),

    #[error("missing field '{0}' in SAM reply")]
    MissingField(&'static str),
}

type Result<T> = std::result::Result<T, SamError>;

const SAM_TCP_ADDR: &str = "127.0.0.1:7656";

struct Control {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

pub struct SamSession {
    pub session_id: String,
    pub destination: String,
    pub b32_addr: String,
    pub udp: Arc<UdpSocket>,
    control: Mutex<Control>,
}

impl SamSession {
    pub async fn new(session_id: &str) -> Result<Self> {
        Self::from_addr(
            session_id,
            &std::env::var("SAM").unwrap_or_else(|_| SAM_TCP_ADDR.to_string()),
        )
        .await
    }

    async fn from_addr(session_id: &str, sam_addr: &str) -> Result<Self> {
        info!("Connecting to SAM bridge at {}", sam_addr);

        let udp_host = std::env::var("UDP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let udp_port = std::env::var("UDP_PORT").unwrap_or_else(|_| "5000".to_string());
        let udp_addr = format!("{}:{}", udp_host, udp_port);

        info!("...binding udp socket: {}", udp_addr);
        let udp_socket = UdpSocket::bind(&udp_addr).await?;

        let stream = TcpStream::connect(sam_addr).await?;
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        send(&mut writer, "HELLO VERSION MIN=3.0 MAX=3.1\n").await?;
        let reply = read_line(&mut reader).await?;
        ensure_ok(&reply)?;

        send(
            &mut writer,
            &format!(
                "SESSION CREATE STYLE=DATAGRAM ID={} DESTINATION=TRANSIENT HOST={} PORT={}\n",
                session_id, udp_host, udp_port
            ),
        )
        .await?;
        let reply = read_line(&mut reader).await?;
        ensure_ok(&reply)?;

        let raw_dest =
            parse_field(&reply, "DESTINATION").ok_or(SamError::MissingField("DESTINATION"))?;

        let destination = lookup(&mut writer, &mut reader, &raw_dest).await?;
        let b32_addr = calculate_b32_address(&destination)?;

        let session = Self {
            session_id: session_id.to_string(),
            destination,
            b32_addr,
            udp: Arc::new(udp_socket),
            control: Mutex::new(Control { reader, writer }),
        };

        Ok(session)
    }

    pub async fn get_b64_addr(&self, b32_addr: &str) -> Result<String> {
        let mut control_guard = self.control.lock().await;
        let Control { reader, writer } = &mut *control_guard;

        lookup(writer, reader, b32_addr).await
    }
}

fn calculate_b32_address(destination: &str) -> Result<String> {
    let normalized_dest = destination.replace('~', "/").replace('-', "+");

    let decoded_bytes = general_purpose::STANDARD
        .decode(normalized_dest)
        .map_err(|_| SamError::MissingField("Failed to decode Base64 destination".into()))?;

    let mut hasher = Sha256::new();
    hasher.update(decoded_bytes);
    let hash_result = hasher.finalize();

    let b32_string = BASE32_NOPAD.encode(&hash_result).to_lowercase();
    Ok(format!("{}.b32.i2p", b32_string))
}

async fn lookup(
    writer: &mut OwnedWriteHalf,
    reader: &mut BufReader<OwnedReadHalf>,
    name: &str,
) -> Result<String> {
    send(writer, &format!("NAMING LOOKUP NAME={}\n", name)).await?;
    let reply = read_line(reader).await?;
    ensure_ok(&reply)?;

    let value = parse_field(&reply, "VALUE").ok_or(SamError::MissingField("VALUE"))?;
    Ok(value)
}

async fn send(writer: &mut OwnedWriteHalf, msg: &str) -> Result<()> {
    writer.write_all(msg.as_bytes()).await?;

    Ok(())
}

async fn read_line(reader: &mut BufReader<OwnedReadHalf>) -> Result<String> {
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line).await?;

    if bytes_read == 0 {
        return Err(SamError::Bridge("SAM bridge closed connection".to_string()));
    }

    Ok(line.trim().to_string())
}

fn ensure_ok(reply: &str) -> Result<()> {
    if !reply.contains("RESULT=OK") {
        let msg = parse_field(reply, "MESSAGE").unwrap_or_else(|| reply.to_string());
        return Err(SamError::Bridge(msg));
    }

    Ok(())
}

fn parse_field(reply: &str, field: &str) -> Option<String> {
    reply
        .split_whitespace()
        .find(|s| s.starts_with(&format!("{field}=")))
        .and_then(|s| s.split_once('='))
        .map(|(_, v)| v.to_string())
}
