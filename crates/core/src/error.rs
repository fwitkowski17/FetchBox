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
    /// Unexpected internal failure (database, ...).
    #[error("internal error: {0}")]
    Internal(String),
    /// Engine-level failure (network, disk, ...).
    #[error("engine error: {0}")]
    Engine(String),
}

/// Result alias for core operations.
pub type Result<T> = std::result::Result<T, Error>;
