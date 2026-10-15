//! Error type shared by core and the engines.

use crate::model::Id;

/// Errors produced by core logic and `Downloader` implementations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No download with this id.
    #[error("download {0} not found")]
    NotFound(Id),
    /// Operation not allowed in the current status.
    #[error("invalid state: {0}")]
    InvalidState(String),
    /// Input (URL, file name, settings) failed validation.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// The new download collides with an existing one; the user must choose.
    #[error("conflict: {0:?}")]
    Conflict(Box<crate::model::Conflict>),
    /// The file will not fit on the disk (with what other running downloads still need).
    #[error("not enough disk space: {} needed, {} available", human_bytes(*.needed), human_bytes(*.available))]
    InsufficientSpace { needed: u64, available: u64 },
    /// Unexpected internal failure (database, ...).
    #[error("internal error: {0}")]
    Internal(String),
    /// Engine-level failure (network, disk, ...).
    #[error("engine error: {0}")]
    Engine(String),
}

/// Result alias for core operations.
pub type Result<T> = std::result::Result<T, Error>;

/// `1.5 GB`-style size for messages (decimal units, like file managers and `df -H`).
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_a_file_manager() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1_000), "1.0 KB");
        assert_eq!(human_bytes(1_500_000), "1.5 MB");
        assert_eq!(human_bytes(4_200_000_000), "4.2 GB");
        assert_eq!(human_bytes(u64::MAX), "18446744.1 TB");
    }

    #[test]
    fn insufficient_space_message_names_both_sizes() {
        let e = Error::InsufficientSpace {
            needed: 4_200_000_000,
            available: 1_100_000_000,
        };
        assert_eq!(
            e.to_string(),
            "not enough disk space: 4.2 GB needed, 1.1 GB available"
        );
    }
}
