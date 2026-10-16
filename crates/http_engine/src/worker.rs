//! Per-segment worker: opens the source at an offset, rate limiting, positional writes.
//!
//! What the bytes come from (an HTTP response, an FTP data connection, an SFTP file) is a
//! [`ChunkSource`]; writing them to the `.part` file, counting and limiting is the same for all.

use std::io::SeekFrom;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use fetchbox_core::limiter::DownloadLimiter;
use fetchbox_core::Credentials;
use reqwest::header::RANGE;
use reqwest::{Client, StatusCode, Url};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::error::EngineError;
use crate::hostkeys::HostKeys;
use crate::policy::UrlPolicy;
use crate::source::Address;
use crate::{ftp, sftp};

const MAX_RETRIES: u32 = 3;

/// The bytes of a file, in the order they arrive.
#[async_trait]
pub trait ChunkSource: Send {
    /// The next piece, or `None` at the end.
    async fn next(&mut self) -> Result<Option<Bytes>, EngineError>;
    /// The stream is no longer needed. `complete`: everything that was asked for has arrived (the
    /// connection can be ended politely); otherwise it is dropped as it is.
    async fn close(&mut self, _complete: bool) {}
}

/// Where the bytes of a download come from.
#[derive(Clone)]
pub enum Transport {
    Http {
        client: Client,
        url: Url,
        login: Option<Credentials>,
    },
    Ftp {
        policy: UrlPolicy,
        target: ftp::Target,
        address: Address,
    },
    Sftp {
        policy: UrlPolicy,
        target: sftp::Target,
        address: Address,
        keys: Arc<dyn HostKeys>,
    },
}

impl Transport {
    /// Open the file at byte `from`. `to` is the last byte wanted (inclusive), where the protocol
    /// can say so; the others just send on and the caller stops reading.
    async fn open(&self, from: u64, to: Option<u64>) -> Result<Box<dyn ChunkSource>, EngineError> {
        match self {
            Transport::Http { client, url, login } => {
                let mut req = client.get(url.clone());
                if let Some(c) = login {
                    req = req.basic_auth(&c.username, Some(&c.password));
                }
                // With an end it is a ranged request; without, a plain stream from the start.
                if let Some(to) = to {
                    req = req.header(RANGE, format!("bytes={from}-{to}"));
                }
                let resp = req.send().await?;
                let status = resp.status();
                let expected_status = if to.is_some() {
                    StatusCode::PARTIAL_CONTENT
                } else {
                    StatusCode::OK
                };
                if status != expected_status {
                    // A 200 for a ranged request means the server ignored `Range`; writing it at
                    // an offset would corrupt the file.
                    return Err(if status.is_success() {
                        EngineError::Protocol(format!("unexpected status {status}"))
                    } else {
                        EngineError::Status(status.as_u16())
                    });
                }
                Ok(Box::new(HttpChunks { resp }))
            }
            Transport::Ftp {
                policy,
                target,
                address,
            } => Ok(Box::new(ftp::open(*policy, target, address, from).await?)),
            Transport::Sftp {
                policy,
                target,
                address,
                keys,
            } => Ok(Box::new(
                sftp::open(*policy, target, address, Arc::clone(keys), from).await?,
            )),
        }
    }
}

struct HttpChunks {
    resp: reqwest::Response,
}

#[async_trait]
impl ChunkSource for HttpChunks {
    async fn next(&mut self) -> Result<Option<Bytes>, EngineError> {
        Ok(self.resp.chunk().await?)
    }
}

/// What one worker is responsible for.
#[derive(Debug, Clone, Copy)]
pub struct SegmentSpec {
    /// First byte of the segment in the file.
    pub start: u64,
    /// Segment length, `None` when the server did not tell us the size.
    pub len: Option<u64>,
    /// Use `Range` requests. `false` = single connection from byte 0, not resumable.
    pub ranged: bool,
}

/// Download one segment into the (already existing) `.part` file.
///
/// `done` counts bytes the operating system has accepted: it is only advanced *after* a write
/// has completed (not merely been buffered), so after an abort or a kill it is a safe resume
/// point for the process. Making those bytes survive a power cut is the engine's checkpoint
/// (`sync`). Retries continue from `done`.
pub async fn run(
    transport: &Transport,
    part: &Path,
    spec: SegmentSpec,
    done: &AtomicU64,
    limiter: &DownloadLimiter,
) -> Result<(), EngineError> {
    let mut attempt = 0;
    loop {
        let result = if spec.ranged {
            let already = done.load(Ordering::Relaxed);
            match spec.len {
                Some(len) if already >= len => return Ok(()),
                Some(len) => {
                    let from = spec.start + already;
                    let to = spec.start + len - 1;
                    fetch(
                        transport,
                        part,
                        Some((from, to)),
                        from,
                        Some(len - already),
                        done,
                        limiter,
                    )
                    .await
                }
                // Ranged requests always come with a known size (see `plan`).
                None => Err(EngineError::Protocol(
                    "ranged segment without length".into(),
                )),
            }
        } else {
            // Cannot resume a plain stream: start over.
            done.store(0, Ordering::Relaxed);
            fetch(transport, part, None, 0, spec.len, done, limiter).await
        };

        match result {
            Ok(()) => return Ok(()),
            Err(e) if e.is_retryable() && attempt < MAX_RETRIES => {
                tracing::warn!(error = %e, attempt, "segment failed, retrying");
                tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn fetch(
    transport: &Transport,
    part: &Path,
    range: Option<(u64, u64)>,
    write_at: u64,
    expected: Option<u64>,
    done: &AtomicU64,
    limiter: &DownloadLimiter,
) -> Result<(), EngineError> {
    let mut source = transport.open(write_at, range.map(|(_, to)| to)).await?;
    let result = pump(&mut *source, part, write_at, expected, done, limiter).await;
    source.close(result.is_ok()).await;
    result
}

/// Write what `source` sends into the `.part` file from `write_at`, until `expected` bytes have
/// come or the source ends.
async fn pump(
    source: &mut dyn ChunkSource,
    part: &Path,
    write_at: u64,
    expected: Option<u64>,
    done: &AtomicU64,
    limiter: &DownloadLimiter,
) -> Result<(), EngineError> {
    let mut file = OpenOptions::new().write(true).open(part).await?;
    file.seek(SeekFrom::Start(write_at)).await?;

    let mut received = 0u64;
    while expected != Some(received) {
        let Some(mut chunk) = source.next().await? else {
            break;
        };
        if let Some(exp) = expected {
            // Never write past the segment, even if the server sends too much.
            let left = usize::try_from(exp - received).unwrap_or(usize::MAX);
            chunk.truncate(left);
        }
        let n = chunk.len() as u64;
        limiter.acquire(n).await;
        file.write_all(&chunk).await?;
        // tokio's file hands the data to a blocking task and returns at once; wait for that
        // write, so that `done` never counts bytes that a killed process would still lose.
        file.flush().await?;
        done.fetch_add(n, Ordering::Relaxed);
        received += n;
    }
    file.flush().await?;

    match expected {
        Some(exp) if received < exp => Err(EngineError::Truncated),
        _ => Ok(()),
    }
}
