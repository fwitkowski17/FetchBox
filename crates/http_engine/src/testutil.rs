//! Minimal range-aware HTTP/1.1 server for engine tests.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Serves one deterministic body at `/file.bin` (anything under `/missing` is a 404).
pub struct TestServer {
    addr: SocketAddr,
    body: Arc<Vec<u8>>,
}

impl TestServer {
    pub async fn spawn(len: usize, ranges: bool, chunk_delay: Duration) -> Self {
        let body: Arc<Vec<u8>> = Arc::new((0..len).map(|i| (i % 251) as u8).collect());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let served = Arc::clone(&body);
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    return;
                };
                let body = Arc::clone(&served);
                tokio::spawn(async move {
                    let _ = handle(sock, &body, ranges, chunk_delay).await;
                });
            }
        });
        Self { addr, body }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{path}", self.addr)
    }

    pub fn len(&self) -> usize {
        self.body.len()
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

async fn handle(
    mut sock: TcpStream,
    body: &[u8],
    ranges: bool,
    delay: Duration,
) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let req = String::from_utf8_lossy(&buf).into_owned();
    let path = req.split_whitespace().nth(1).unwrap_or("/");
    if path.starts_with("/missing") {
        sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }

    let total = body.len();
    let range = req
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("range: bytes=")
                .map(str::to_owned)
        })
        .and_then(|r| {
            let (a, b) = r.trim().split_once('-')?;
            let a: usize = a.parse().ok()?;
            let b: usize = b.parse().unwrap_or(total - 1);
            Some((a, b.min(total - 1)))
        });

    let (status, from, to, extra) = match range {
        Some((a, b)) if ranges => (
            "206 Partial Content",
            a,
            b,
            format!("Content-Range: bytes {a}-{b}/{total}\r\n"),
        ),
        _ => ("200 OK", 0, total - 1, String::new()),
    };
    let slice = &body[from..=to];
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nETag: \"v1\"\r\n{extra}Accept-Ranges: {}\r\nConnection: close\r\n\r\n",
        slice.len(),
        if ranges { "bytes" } else { "none" },
    );
    sock.write_all(head.as_bytes()).await?;
    for chunk in slice.chunks(32 * 1024) {
        sock.write_all(chunk).await?;
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
    Ok(())
}
