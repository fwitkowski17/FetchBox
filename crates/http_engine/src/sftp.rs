//! SFTP (SPEC 6.12): the SSH file transfer protocol, over `russh`.
//!
//! The SSH side is the part that needs care. A server is believed because its host key is one the
//! user accepted (`HostKeys`), never on first sight: an unknown or changed key stops the
//! connection and is reported with the fingerprint, so the user can compare it and decide. Only
//! password logins exist for now.
//!
//! Reading is pipelined by the SFTP library (several read requests in flight), which is what makes
//! one connection fast on a line with some latency.

use std::io::SeekFrom;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use fetchbox_core::{Credentials, HostKeyProblem, HostKeyProblemKind};
use russh::client;
use russh::keys::{HashAlg, PublicKeyOrCertificate};
use russh::Disconnect;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::fs::File;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::StatusCode;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::time::timeout;

use crate::error::EngineError;
use crate::hostkeys::HostKeys;
use crate::listing::{from_raw, RawEntry};
use crate::net;
use crate::policy::UrlPolicy;
use crate::source::{Address, Inspected};
use crate::worker::ChunkSource;

/// How long the key exchange and the login may take.
const SETUP_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a transfer may stand still before it counts as broken.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// The size of one read from the file.
const CHUNK: usize = 128 * 1024;

/// Where to connect and how to log in.
#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub login: Option<Credentials>,
}

impl Target {
    pub fn new(address: &Address, login: Option<Credentials>) -> Self {
        Self {
            host: address.host(),
            port: address.port(),
            login,
        }
    }
}

/// Errors of the SSH handler: the library's own, or a refused host key (which must reach the user
/// as it is, not as a failed handshake).
#[derive(Debug)]
enum SshError {
    Ssh(russh::Error),
    HostKey(Box<HostKeyProblem>),
}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> Self {
        SshError::Ssh(e)
    }
}

fn ssh_err(e: SshError) -> EngineError {
    match e {
        SshError::HostKey(problem) => EngineError::HostKey(problem),
        SshError::Ssh(e) => EngineError::Network(format!("SSH: {e}")),
    }
}

struct Handler {
    host: String,
    port: u16,
    keys: Arc<dyn HostKeys>,
}

impl client::Handler for Handler {
    type Error = SshError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, SshError> {
        let key = server_public_key.public_key();
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let problem = |kind, trusted_fingerprint| {
            SshError::HostKey(Box::new(HostKeyProblem {
                kind,
                host: self.host.clone(),
                port: self.port,
                key_type: key.algorithm().to_string(),
                fingerprint: fingerprint.clone(),
                trusted_fingerprint,
            }))
        };
        match self.keys.trusted(&self.host, self.port).await {
            Some(trusted) if trusted == fingerprint => Ok(true),
            Some(trusted) => Err(problem(HostKeyProblemKind::Changed, Some(trusted))),
            None => Err(problem(HostKeyProblemKind::Untrusted, None)),
        }
    }
}

/// A logged-in SFTP connection.
pub struct Session {
    sftp: SftpSession,
    handle: client::Handle<Handler>,
}

fn sftp_err(e: SftpError) -> EngineError {
    match &e {
        SftpError::Status(s) if matches!(s.status_code, StatusCode::NoSuchFile) => {
            EngineError::NotFound(format!("not on the server: {}", s.error_message))
        }
        SftpError::Status(s) if matches!(s.status_code, StatusCode::PermissionDenied) => {
            EngineError::NotFound(format!("not allowed to read it: {}", s.error_message))
        }
        _ => EngineError::Network(format!("SFTP: {e}")),
    }
}

/// Connect, check the host key, log in and start the SFTP subsystem.
pub async fn connect(
    policy: UrlPolicy,
    keys: Arc<dyn HostKeys>,
    target: &Target,
) -> Result<Session, EngineError> {
    let Some(login) = &target.login else {
        return Err(EngineError::Auth(
            "SFTP needs a user name and a password".into(),
        ));
    };
    let tcp = net::connect(policy, &target.host, target.port).await?;
    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..client::Config::default()
    });
    let handler = Handler {
        host: target.host.clone(),
        port: target.port,
        keys,
    };
    let mut handle = timeout(SETUP_TIMEOUT, client::connect_stream(config, tcp, handler))
        .await
        .map_err(|_| EngineError::Network("the SSH handshake timed out".into()))?
        .map_err(ssh_err)?;

    let auth = timeout(
        SETUP_TIMEOUT,
        handle.authenticate_password(login.username.as_str(), login.password.as_str()),
    )
    .await
    .map_err(|_| EngineError::Network("the login timed out".into()))?
    .map_err(|e| EngineError::Network(format!("SSH: {e}")))?;
    if !auth.success() {
        return Err(EngineError::Auth(
            "the server refused the user name or password (does it allow password logins?)".into(),
        ));
    }

    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| EngineError::Network(format!("SSH: {e}")))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| EngineError::Network(format!("the server has no SFTP: {e}")))?;
    let cfg = russh_sftp::client::Config {
        request_timeout_secs: 30,
        ..russh_sftp::client::Config::default()
    };
    let sftp = SftpSession::new_with_config(channel.into_stream(), cfg)
        .await
        .map_err(sftp_err)?;
    Ok(Session { sftp, handle })
}

impl Session {
    async fn close(&self) {
        let _ = self.sftp.close().await;
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;
    }
}

/// The path of an address as SFTP wants it: absolute (`sftp://host/srv/a` is `/srv/a`), or
/// relative to the home folder after `/~/` (`sftp://host/~/a` is `a`), as curl reads it.
pub fn remote_path(address: &Address) -> String {
    let path = address.path();
    match path.strip_prefix("/~") {
        Some("") => ".".to_owned(),
        Some(rest) if rest.starts_with('/') => rest[1..].to_owned(),
        _ if path.is_empty() => "/".to_owned(),
        _ => path,
    }
}

/// Look at an address: a file (size, date) or a folder (its list).
pub async fn inspect(
    policy: UrlPolicy,
    keys: Arc<dyn HostKeys>,
    target: &Target,
    address: &Address,
) -> Result<Inspected, EngineError> {
    let session = connect(policy, keys, target).await?;
    let result = inspect_inner(&session, address).await;
    session.close().await;
    result
}

async fn inspect_inner(session: &Session, address: &Address) -> Result<Inspected, EngineError> {
    let path = remote_path(address);
    let meta = session
        .sftp
        .metadata(path.as_str())
        .await
        .map_err(sftp_err)?;
    if !meta.is_dir() {
        return Ok(Inspected::File {
            size: meta.size,
            modified: meta.mtime.map(|t| t.to_string()),
            resumable: meta.size.is_some(),
        });
    }
    let mut raw = Vec::new();
    let entries = session
        .sftp
        .read_dir(path.as_str())
        .await
        .map_err(sftp_err)?;
    for entry in entries {
        let name = entry.file_name();
        let meta = entry.metadata();
        // A link tells nothing about what it points to: ask for that.
        let meta = if entry.file_type().is_symlink() {
            match session.sftp.metadata(entry.path()).await {
                Ok(m) => m,
                Err(_) => continue,
            }
        } else {
            meta
        };
        if meta.is_dir() {
            raw.push(RawEntry {
                name,
                size: None,
                is_dir: true,
            });
        } else if meta.file_type().is_file() {
            raw.push(RawEntry {
                name,
                size: meta.size,
                is_dir: false,
            });
        }
    }
    Ok(Inspected::Dir(from_raw(&address.url, raw)))
}

/// A file on its way, as the download worker wants it.
pub struct Chunks {
    file: File,
    session: Session,
    buf: Vec<u8>,
}

/// Open `address` at byte `offset` for the download worker.
pub async fn open(
    policy: UrlPolicy,
    target: &Target,
    address: &Address,
    keys: Arc<dyn HostKeys>,
    offset: u64,
) -> Result<Chunks, EngineError> {
    let session = connect(policy, keys, target).await?;
    let path = remote_path(address);
    let mut file = session.sftp.open(path).await.map_err(sftp_err)?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset))
            .await
            .map_err(|e| EngineError::Network(format!("cannot seek in the remote file: {e}")))?;
    }
    Ok(Chunks {
        file,
        session,
        buf: vec![0; CHUNK],
    })
}

#[async_trait]
impl ChunkSource for Chunks {
    async fn next(&mut self) -> Result<Option<Bytes>, EngineError> {
        let n = match timeout(IDLE_TIMEOUT, self.file.read(&mut self.buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                return Err(EngineError::Network(format!(
                    "reading from the server failed: {e}"
                )))
            }
            Err(_) => {
                return Err(EngineError::Network(
                    "no data from the server for 30 seconds".into(),
                ))
            }
        };
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(Bytes::copy_from_slice(&self.buf[..n])))
    }

    async fn close(&mut self, _complete: bool) -> Result<(), EngineError> {
        self.session.close().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::parse;

    #[test]
    fn paths_are_absolute_unless_they_start_in_the_home_folder() {
        let p = |raw| remote_path(&parse(raw).expect("url"));
        assert_eq!(p("sftp://h/srv/a%20b.iso"), "/srv/a b.iso");
        assert_eq!(p("sftp://h/~/Downloads/a.iso"), "Downloads/a.iso");
        assert_eq!(p("sftp://h/~"), ".");
        assert_eq!(p("sftp://h/"), "/");
        assert_eq!(p("sftp://h"), "/");
        // Only a folder called exactly `~` is the home folder.
        assert_eq!(p("sftp://h/~user/a"), "/~user/a");
    }
}
