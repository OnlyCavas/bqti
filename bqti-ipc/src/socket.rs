use std::{io, path::Path};

use futures::Stream;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::{Event, Reply, Request, socket_path};

pub const SOCKET_PATH: &str = "BQTI_SOCKET";

pub struct Socket {
    stream: BufReader<UnixStream>,
}

impl Socket {
    async fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let stream = UnixStream::connect(path.as_ref()).await?;

        Ok(Self {
            stream: BufReader::new(stream),
        })
    }

    pub async fn connect() -> io::Result<Self> {
        let path = socket_path();
        Self::new(path).await
    }

    pub async fn send(&mut self, request: Request) -> io::Result<Reply> {
        let mut buf = serde_json::to_string(&request).map_err(io::Error::other)?;
        buf.push('\n');
        self.stream.write_all(buf.as_bytes()).await?;

        buf.clear();
        self.stream.read_line(&mut buf).await?;
        serde_json::from_str(&buf).map_err(io::Error::other)
    }

    pub fn event_stream(self) -> impl Stream<Item = io::Result<Event>> {
        let mut stream = self.stream;
        let _ = stream.get_mut().shutdown();

        futures::stream::unfold(stream, |mut stream| async move {
            let mut buf = String::new();

            match stream.read_line(&mut buf).await {
                Ok(0) => None,
                Ok(_) => Some((serde_json::from_str(&buf).map_err(io::Error::other), stream)),
                Err(e) => Some((Err(e), stream)),
            }
        })
    }
}
