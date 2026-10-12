//! Two-level token bucket: global and per-download (SPEC 6.3).
//! Placeholder for milestone 0.

/// Rate limiter handed to every segment worker.
#[derive(Debug, Default)]
pub struct RateLimiter;

impl RateLimiter {
    /// Create an unlimited limiter.
    pub fn new() -> Self {
        Self
    }
}
