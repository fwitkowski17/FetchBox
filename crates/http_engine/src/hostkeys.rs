//! Which SSH servers are trusted (SPEC 6.12). SFTP has no certificate authorities: a server is
//! believed because its host key is the one the user accepted, so the engine asks whoever owns
//! the list (the server wiring keeps it in the database).

use async_trait::async_trait;

/// The accepted host keys.
#[async_trait]
pub trait HostKeys: Send + Sync {
    /// The fingerprint (`SHA256:...`) accepted for `host:port`, if the user accepted one.
    async fn trusted(&self, host: &str, port: u16) -> Option<String>;
}

/// Trusts nothing: every SSH server is refused as unknown. What the engine uses until it is given
/// a list, and in tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHostKeys;

#[async_trait]
impl HostKeys for NoHostKeys {
    async fn trusted(&self, _host: &str, _port: u16) -> Option<String> {
        None
    }
}
