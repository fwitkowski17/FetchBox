//! Plugin machinery that needs no database and no HTTP server (SPEC 6.15): unpacking archives,
//! installing a plugin's files, and (in later modules) fetching dependencies and running a plugin's
//! programs. The manifest types and the pure rules live in `fetchbox_core::plugin`.

pub mod archive;
pub mod deps;
pub mod expand;
pub mod fetch;
pub mod install;
pub mod mapping;
pub mod oneshot;
pub mod proc;
pub mod process;
pub mod protocol;
pub mod store;
pub mod webhook;

use std::io;

/// Errors of the plugin machinery.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// The archive is damaged or unsafe (a path that escapes, a link, a limit exceeded).
    #[error("{0}")]
    Archive(String),
    /// `plugin.json` is missing, unreadable or invalid.
    #[error("{0}")]
    Manifest(String),
    /// A download failed: the address is not allowed, the server refused, the file is the wrong
    /// size, or its checksum does not match.
    #[error("{0}")]
    Fetch(String),
    /// A command could not run or failed: a template cannot be filled in, the program is missing,
    /// it timed out, or it exited with an error.
    #[error("{0}")]
    Run(String),
    /// The thing asked for is not there (an unknown staged token, a plugin that is not installed).
    #[error("{0}")]
    NotFound(String),
}

impl From<Error> for fetchbox_core::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Archive(m) | Error::Manifest(m) => fetchbox_core::Error::InvalidInput(m),
            Error::NotFound(m) => fetchbox_core::Error::Missing(m),
            Error::Fetch(m) | Error::Run(m) => fetchbox_core::Error::Engine(m),
            Error::Io(e) => fetchbox_core::Error::Internal(e.to_string()),
        }
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
