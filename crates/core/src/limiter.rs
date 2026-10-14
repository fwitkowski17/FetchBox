//! Two-level token bucket: global and per-download (SPEC 6.3).
//!
//! Every segment worker calls [`DownloadLimiter::acquire`] before reading from the
//! socket. Rates can be changed at any time and apply to running downloads.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

/// Token bucket with a one-second burst. A rate of `0` means unlimited.
#[derive(Debug)]
pub struct Bucket {
    state: Mutex<BucketState>,
}

#[derive(Debug)]
struct BucketState {
    /// Bytes per second, `0` = unlimited.
    rate: u64,
    tokens: f64,
    last: Instant,
}

impl Bucket {
    /// Create a bucket limited to `rate` bytes/s (`0` = unlimited).
    pub fn new(rate: u64) -> Self {
        Self {
            state: Mutex::new(BucketState {
                rate,
                tokens: rate as f64,
                last: Instant::now(),
            }),
        }
    }

    /// Change the rate; takes effect for the next `acquire`.
    pub fn set_rate(&self, rate: u64) {
        let mut s = self.lock();
        refill(&mut s);
        s.rate = rate;
        s.tokens = s.tokens.min(rate as f64);
    }

    /// Current rate in bytes/s (`0` = unlimited).
    pub fn rate(&self) -> u64 {
        self.lock().rate
    }

    /// Wait until `bytes` tokens are available and take them.
    pub async fn acquire(&self, bytes: u64) {
        let mut remaining = bytes;
        while remaining > 0 {
            let wait = {
                let mut s = self.lock();
                if s.rate == 0 {
                    return;
                }
                refill(&mut s);
                // Never ask for more than the bucket can hold, or we would wait forever.
                let take = (remaining as f64).min(s.rate as f64);
                if s.tokens >= take {
                    s.tokens -= take;
                    remaining -= take as u64;
                    continue;
                }
                Duration::from_secs_f64((take - s.tokens) / s.rate as f64)
            };
            tokio::time::sleep(wait).await;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BucketState> {
        // The state is plain data; a poisoned lock is still consistent.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn refill(s: &mut BucketState) {
    let now = Instant::now();
    let elapsed = now.duration_since(s.last).as_secs_f64();
    s.last = now;
    s.tokens = (s.tokens + elapsed * s.rate as f64).min(s.rate as f64);
}

/// Global limiter shared by all downloads.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    global: Arc<Bucket>,
}

impl RateLimiter {
    /// Create an unlimited limiter.
    pub fn new() -> Self {
        Self::with_global(0)
    }

    /// Create a limiter with a global limit in bytes/s (`0` = unlimited).
    pub fn with_global(rate: u64) -> Self {
        Self {
            global: Arc::new(Bucket::new(rate)),
        }
    }

    /// Update the global limit live.
    pub fn set_global(&self, rate: u64) {
        self.global.set_rate(rate);
    }

    /// Create the per-download limiter for one download.
    pub fn for_download(&self, per_download_rate: u64) -> DownloadLimiter {
        DownloadLimiter {
            own: Arc::new(Bucket::new(per_download_rate)),
            global: Arc::clone(&self.global),
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Limiter for one download: its own bucket, then the global one.
/// Cheap to clone; all segments of a download share the same instance.
#[derive(Debug, Clone)]
pub struct DownloadLimiter {
    own: Arc<Bucket>,
    global: Arc<Bucket>,
}

impl DownloadLimiter {
    /// Update the per-download limit live.
    pub fn set_rate(&self, rate: u64) {
        self.own.set_rate(rate);
    }

    /// Wait for permission to read `bytes` from the network.
    pub async fn acquire(&self, bytes: u64) {
        self.own.acquire(bytes).await;
        self.global.acquire(bytes).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn unlimited_never_waits() {
        let start = Instant::now();
        let l = RateLimiter::new().for_download(0);
        l.acquire(10_000_000).await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn burst_is_one_second_then_throttled() {
        let b = Bucket::new(1000);
        let start = Instant::now();
        b.acquire(1000).await; // burst
        assert_eq!(start.elapsed(), Duration::ZERO);
        b.acquire(500).await;
        let e = start.elapsed();
        assert!(
            e >= Duration::from_millis(499) && e <= Duration::from_millis(600),
            "{e:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn request_larger_than_bucket_completes() {
        let b = Bucket::new(100);
        let start = Instant::now();
        b.acquire(1000).await;
        // 100 burst + 900 at 100 B/s.
        let e = start.elapsed();
        assert!(
            e >= Duration::from_secs(8) && e <= Duration::from_secs(10),
            "{e:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn global_limit_applies_across_downloads() {
        let rl = RateLimiter::with_global(1000);
        let a = rl.for_download(0);
        let b = rl.for_download(0);
        let start = Instant::now();
        a.acquire(1000).await;
        b.acquire(1000).await;
        let e = start.elapsed();
        assert!(e >= Duration::from_millis(990), "{e:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn rate_change_applies_live() {
        let b = Bucket::new(100);
        b.acquire(100).await;
        b.set_rate(0);
        let start = Instant::now();
        b.acquire(1_000_000).await;
        assert_eq!(start.elapsed(), Duration::ZERO);
        assert_eq!(b.rate(), 0);
    }
}
