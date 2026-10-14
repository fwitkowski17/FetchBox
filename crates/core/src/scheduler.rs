//! Scheduler: FIFO queue with a concurrency limit (SPEC 6.3).
//!
//! Pure state machine: it only decides *which* downloads may run. The caller
//! (server wiring) starts/stops them through the `Downloader` and updates statuses.
//! The limit can be changed at any time without touching running downloads.

use std::collections::{HashSet, VecDeque};

use crate::model::Id;

/// Decides which downloads run and which stay `Queued`.
#[derive(Debug)]
pub struct Scheduler {
    max_concurrent: usize,
    queue: VecDeque<Id>,
    running: HashSet<Id>,
}

impl Scheduler {
    /// Create an empty scheduler. A limit of `0` is treated as `1`.
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            max_concurrent: limit(max_concurrent),
            queue: VecDeque::new(),
            running: HashSet::new(),
        }
    }

    /// Add a download to the back of the queue (no-op if already known).
    /// Returns downloads that should start now.
    pub fn enqueue(&mut self, id: Id) -> Vec<Id> {
        if !self.running.contains(&id) && !self.queue.contains(&id) {
            self.queue.push_back(id);
        }
        self.fill()
    }

    /// A running download ended (completed, failed, paused, cancelled) or a queued
    /// one was removed. Returns downloads that should start now.
    pub fn release(&mut self, id: Id) -> Vec<Id> {
        self.running.remove(&id);
        self.queue.retain(|q| *q != id);
        self.fill()
    }

    /// Apply a new `max_concurrent` live. Returns downloads that should start now;
    /// lowering the limit never stops running downloads, it only delays new ones.
    pub fn set_max_concurrent(&mut self, max: u32) -> Vec<Id> {
        self.max_concurrent = limit(max);
        self.fill()
    }

    /// Whether `id` currently occupies a slot.
    pub fn is_running(&self, id: Id) -> bool {
        self.running.contains(&id)
    }

    /// Number of downloads waiting for a slot.
    pub fn queued_len(&self) -> usize {
        self.queue.len()
    }

    /// Number of downloads occupying a slot.
    pub fn running_len(&self) -> usize {
        self.running.len()
    }

    fn fill(&mut self) -> Vec<Id> {
        let mut started = Vec::new();
        while self.running.len() < self.max_concurrent {
            let Some(id) = self.queue.pop_front() else {
                break;
            };
            self.running.insert(id);
            started.push(id);
        }
        started
    }
}

fn limit(n: u32) -> usize {
    (n as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respects_limit_and_fifo_order() {
        let mut s = Scheduler::new(2);
        assert_eq!(s.enqueue(1), vec![1]);
        assert_eq!(s.enqueue(2), vec![2]);
        assert!(s.enqueue(3).is_empty());
        assert!(s.enqueue(4).is_empty());
        assert_eq!(s.queued_len(), 2);
        assert_eq!(s.release(1), vec![3]);
        assert_eq!(s.release(2), vec![4]);
        assert_eq!(s.running_len(), 2);
    }

    #[test]
    fn raising_limit_starts_queued_downloads() {
        let mut s = Scheduler::new(1);
        s.enqueue(1);
        s.enqueue(2);
        s.enqueue(3);
        assert_eq!(s.set_max_concurrent(3), vec![2, 3]);
    }

    #[test]
    fn lowering_limit_keeps_running_but_delays_new() {
        let mut s = Scheduler::new(3);
        for id in 1..=3 {
            s.enqueue(id);
        }
        assert!(s.set_max_concurrent(1).is_empty());
        assert_eq!(s.running_len(), 3);
        s.enqueue(4);
        assert!(s.release(1).is_empty());
        assert!(s.release(2).is_empty());
        assert_eq!(s.release(3), vec![4]);
    }

    #[test]
    fn releasing_queued_download_removes_it() {
        let mut s = Scheduler::new(1);
        s.enqueue(1);
        s.enqueue(2);
        assert!(s.release(2).is_empty());
        assert_eq!(s.queued_len(), 0);
    }

    #[test]
    fn duplicate_enqueue_is_ignored() {
        let mut s = Scheduler::new(1);
        assert_eq!(s.enqueue(1), vec![1]);
        assert!(s.enqueue(1).is_empty());
        assert_eq!(s.running_len(), 1);
    }

    #[test]
    fn zero_limit_is_clamped_to_one() {
        let mut s = Scheduler::new(0);
        assert_eq!(s.enqueue(1), vec![1]);
    }
}
