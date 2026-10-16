//! Per-segment worker: ranged GET, rate limiting, positional writes.

use std::io::SeekFrom;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fetchbox_core::limiter::DownloadLimiter;
use futures_util::StreamExt;
use reqwest::header::RANGE;
use reqwest::{Client, StatusCode, Url};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::error::EngineError;

const MAX_RETRIES: u32 = 3;

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
    client: &Client,
    url: &Url,
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
                        client,
                        url,
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
            fetch(client, url, part, None, 0, spec.len, done, limiter).await
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
    client: &Client,
    url: &Url,
    part: &Path,
    range: Option<(u64, u64)>,
    write_at: u64,
    expected: Option<u64>,
    done: &AtomicU64,
    limiter: &DownloadLimiter,
) -> Result<(), EngineError> {
    let mut req = client.get(url.clone());
    if let Some((from, to)) = range {
        req = req.header(RANGE, format!("bytes={from}-{to}"));
    }
    let resp = req.send().await?;
    let status = resp.status();
    let expected_status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    if status != expected_status {
        // A 200 for a ranged request means the server ignored `Range`; writing it at an
        // offset would corrupt the file.
        return Err(if status.is_success() {
            EngineError::Protocol(format!("unexpected status {status}"))
        } else {
            EngineError::Status(status.as_u16())
        });
    }

    let mut file = OpenOptions::new().write(true).open(part).await?;
    file.seek(SeekFrom::Start(write_at)).await?;

    let mut stream = resp.bytes_stream();
    let mut received = 0u64;
    while expected != Some(received) {
        let Some(chunk) = stream.next().await else {
            break;
        };
        let mut chunk = chunk?;
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
