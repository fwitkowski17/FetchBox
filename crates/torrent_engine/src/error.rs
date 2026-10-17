//! Errors of the torrent engine.

/// What can go wrong when handling a torrent. The messages are shown to the user as they are.
#[derive(Debug, thiserror::Error)]
pub enum TorrentError {
    /// The magnet link or the `.torrent` file is not usable.
    #[error("{0}")]
    Invalid(String),
    /// The torrent is already in the session.
    #[error("this torrent has been added already")]
    Duplicate,
    /// No such torrent in the session.
    #[error("no such torrent")]
    NotFound,
    /// The engine could not start or a command failed.
    #[error("{0}")]
    Engine(String),
}

impl From<TorrentError> for fetchbox_core::Error {
    fn from(e: TorrentError) -> Self {
        match e {
            TorrentError::Invalid(m) => fetchbox_core::Error::InvalidInput(m),
            TorrentError::Duplicate => fetchbox_core::Error::InvalidState(e.to_string()),
            TorrentError::NotFound => fetchbox_core::Error::Missing(e.to_string()),
            TorrentError::Engine(m) => fetchbox_core::Error::Engine(m),
        }
    }
}
