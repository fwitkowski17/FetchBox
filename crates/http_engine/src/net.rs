//! Connecting for the protocols that do not go through `reqwest` (FTP, SFTP): resolve the name,
//! keep only the addresses the policy allows (SPEC 6.7, the same rule as for HTTP), connect.

use std::time::Duration;

use tokio::net::TcpStream;

use crate::error::EngineError;
use crate::policy::UrlPolicy;

/// How long a connection may take to be made.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Open a TCP connection to `host:port`. The name is resolved once and only allowed addresses are
/// tried, so a name that resolves to a private address (or is rebound to one) cannot reach it.
pub(crate) async fn connect(
    policy: UrlPolicy,
    host: &str,
    port: u16,
) -> Result<TcpStream, EngineError> {
    let found: Vec<_> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| EngineError::Network(format!("cannot resolve {host}: {e}")))?
        .collect();
    if found.is_empty() {
        return Err(EngineError::Network(format!("{host} has no address")));
    }
    let allowed: Vec<_> = found
        .into_iter()
        .filter(|a| policy.allows(a.ip()))
        .collect();
    if allowed.is_empty() {
        return Err(EngineError::Network(format!(
            "{host} resolves only to blocked (private/local) addresses"
        )));
    }
    let mut last = None;
    for addr in allowed {
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => {
                // Small requests (FTP commands, SFTP packets) must not wait for Nagle.
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Ok(Err(e)) => last = Some(format!("{addr}: {e}")),
            Err(_) => last = Some(format!("{addr}: timed out")),
        }
    }
    Err(EngineError::Network(format!(
        "cannot connect to {host}:{port} ({})",
        last.unwrap_or_default()
    )))
}
