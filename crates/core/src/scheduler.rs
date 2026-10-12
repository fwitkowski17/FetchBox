//! Scheduler: queue, concurrency semaphore, live settings propagation (SPEC 6.3).
//! Placeholder for milestone 0.

/// Decides which downloads run and which stay `Queued`.
#[derive(Debug, Default)]
pub struct Scheduler;

impl Scheduler {
    /// Create an empty scheduler.
    pub fn new() -> Self {
        Self
    }
}
