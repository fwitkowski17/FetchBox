//! A small FTP / FTPS client (SPEC 6.12): log in, ask for size and date, list a folder, fetch a
//! file from an offset. Passive mode only, and one thing at a time on a connection.
//!
//! FTP is a line protocol, so this is a few hundred lines rather than a dependency. The things
//! that matter for a download manager and are easy to get wrong:
//! - a line break in a name would be a second command: names are checked where addresses are
//!   parsed (`source::parse`), and `command` refuses a line with one as well;
//! - the address a server names for the data connection is ignored (`PASV` can point anywhere,
//!   which would let a server aim this program at another machine): data connections go to the
//!   address of the control connection;
//! - with TLS the data connection is protected too (`PROT P`) and verified against the same roots
//!   as HTTPS; a server that does not offer TLS when `ftps://` was asked for is an error, never a
//!   silent downgrade.

use std::net::IpAddr;
use std::sync::Arc;
#[cfg(not(test))]
use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use fetchbox_core::Credentials;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

use crate::error::EngineError;
use crate::listing::RawEntry;
use crate::net;
use crate::policy::UrlPolicy;
use crate::source::{has_control, Address, Inspected, Scheme};
use crate::worker::ChunkSource;

/// How long the server may take to answer a command.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a transfer may stand still before it counts as broken.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest line of a reply that is accepted.
const MAX_LINE: usize = 16 * 1024;
/// Most lines of one reply.
const MAX_REPLY_LINES: usize = 2000;
/// Most bytes of a folder listing that are read.
const MAX_LISTING_BYTES: usize = 8 * 1024 * 1024;
/// The size of one read from the data connection.
const CHUNK: usize = 64 * 1024;

trait Io: AsyncRead + AsyncWrite + Unpin + Send + Sync {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> Io for T {}
type BoxIo = Box<dyn Io>;

/// How the connection is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tls {
    /// Not at all (`ftp://`).
    None,
    /// `AUTH TLS` on the usual port (`ftps://host/`).
    Explicit,
    /// TLS from the first byte, as on port 990 (`ftps://host:990/`).
    Implicit,
}

/// Where to connect and how to log in.
#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub tls: Tls,
    /// `None` logs in as `anonymous`.
    pub login: Option<Credentials>,
}

impl Target {
    pub fn new(address: &Address, login: Option<Credentials>) -> Self {
        let port = address.port();
        let tls = match address.scheme {
            Scheme::Ftps if port == 990 => Tls::Implicit,
            Scheme::Ftps => Tls::Explicit,
            _ => Tls::None,
        };
        Self {
            host: address.host(),
            port,
            tls,
            login,
        }
    }
}

/// One reply of the server: a three-digit code and its text (several lines joined with `\n`).
#[derive(Debug, Clone)]
struct Reply {
    code: u16,
    text: String,
}

impl Reply {
    /// An error carrying this reply.
    fn error(&self) -> EngineError {
        match self.code {
            530 | 532 => EngineError::Auth(self.text.clone()),
            550 | 450 if looks_missing(&self.text) => EngineError::NotFound(self.text.clone()),
            _ => EngineError::Ftp {
                code: self.code,
                message: self.text.clone(),
            },
        }
    }

    /// `self`, or an error unless the code is one of `ok`.
    fn expect(self, ok: &[u16]) -> Result<Self, EngineError> {
        if ok.contains(&self.code) {
            Ok(self)
        } else {
            Err(self.error())
        }
    }
}

fn looks_missing(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("no such") || t.contains("not found") || t.contains("does not exist")
}

/// A logged-in connection.
pub struct Session {
    ctrl: BufReader<BoxIo>,
    /// Where data connections go: the address the control connection reached.
    peer: IpAddr,
    /// Present when data connections are protected.
    tls: Option<(TlsConnector, ServerName<'static>)>,
    /// `MLSD` can be used for folders.
    mlsd: bool,
    /// The server said it can continue from an offset.
    rest: bool,
}

/// Certificates the tests' own servers use, trusted in addition to the usual roots.
#[cfg(test)]
static TEST_ROOTS: std::sync::Mutex<Vec<rustls::pki_types::CertificateDer<'static>>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) fn trust_for_tests(cert: rustls::pki_types::CertificateDer<'static>) {
    TEST_ROOTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(cert);
}

fn build_tls_config() -> Result<rustls::ClientConfig, EngineError> {
    #[allow(unused_mut)]
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    #[cfg(test)]
    for cert in TEST_ROOTS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let _ = roots.add(cert.clone());
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| EngineError::Network(format!("cannot set up TLS: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

#[cfg(not(test))]
fn tls_config() -> Result<Arc<rustls::ClientConfig>, EngineError> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    if let Some(c) = CONFIG.get() {
        return Ok(Arc::clone(c));
    }
    // One config for all connections: it keeps the TLS sessions, so a data connection can resume
    // the session of the control connection (some servers insist on it).
    let config = build_tls_config()?;
    Ok(Arc::clone(CONFIG.get_or_init(|| Arc::new(config))))
}

#[cfg(test)]
fn tls_config() -> Result<Arc<rustls::ClientConfig>, EngineError> {
    // Not shared between tests: each brings its own certificate.
    Ok(Arc::new(build_tls_config()?))
}

async fn handshake(
    connector: &TlsConnector,
    name: &ServerName<'static>,
    io: impl AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
) -> Result<BoxIo, EngineError> {
    let tls = timeout(REPLY_TIMEOUT, connector.connect(name.clone(), io))
        .await
        .map_err(|_| EngineError::Network("the TLS handshake timed out".into()))?
        .map_err(|e| EngineError::Network(format!("TLS handshake failed: {e}")))?;
    Ok(Box::new(tls))
}

/// Connect, secure the connection if asked to, and log in.
pub async fn connect(policy: UrlPolicy, target: &Target) -> Result<Session, EngineError> {
    let tcp = net::connect(policy, &target.host, target.port).await?;
    let peer = tcp
        .peer_addr()
        .map_err(|e| EngineError::Network(e.to_string()))?
        .ip();
    let tls = if target.tls == Tls::None {
        None
    } else {
        let name = ServerName::try_from(target.host.clone())
            .map_err(|e| EngineError::InvalidUrl(format!("bad host name for TLS: {e}")))?;
        Some((TlsConnector::from(tls_config()?), name))
    };
    let io: BoxIo = match (&tls, target.tls) {
        (Some((connector, name)), Tls::Implicit) => handshake(connector, name, tcp).await?,
        _ => Box::new(tcp),
    };
    let mut session = Session {
        ctrl: BufReader::new(io),
        peer,
        tls,
        mlsd: false,
        rest: false,
    };

    let mut greeting = session.read_reply().await?;
    if greeting.code == 120 {
        // "Service ready in nnn minutes": the real greeting follows.
        greeting = session.read_reply().await?;
    }
    greeting.expect(&[220])?;

    if target.tls == Tls::Explicit {
        let r = session.command("AUTH TLS").await?;
        if r.code != 234 {
            return Err(EngineError::Network(format!(
                "the server does not offer TLS ({}); use ftp:// for a plain connection",
                r.text
            )));
        }
        let Some((connector, name)) = session.tls.clone() else {
            return Err(EngineError::Network("TLS is not set up".into()));
        };
        let plain = std::mem::replace(&mut session.ctrl, BufReader::new(Box::new(Closed)));
        session.ctrl = BufReader::new(handshake(&connector, &name, plain.into_inner()).await?);
        session.command("PBSZ 0").await?.expect(&[200])?;
        session.command("PROT P").await?.expect(&[200])?;
    }
    session.login(target.login.as_ref()).await?;
    session.negotiate().await?;
    Ok(session)
}

/// A stand-in while the control connection is being replaced by its TLS version.
struct Closed;

impl AsyncRead for Closed {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Closed {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Err(std::io::ErrorKind::NotConnected.into()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

fn net_err(what: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Network(format!("{what}: {e}"))
}

impl Session {
    async fn read_line(&mut self) -> Result<String, EngineError> {
        let mut buf = Vec::new();
        let n = (&mut self.ctrl)
            .take(MAX_LINE as u64)
            .read_until(b'\n', &mut buf)
            .await
            .map_err(|e| net_err("reading from the server failed", e))?;
        if n == 0 {
            return Err(EngineError::Network(
                "the server closed the connection".into(),
            ));
        }
        if buf.last() != Some(&b'\n') {
            return Err(EngineError::Network("a reply line is too long".into()));
        }
        Ok(String::from_utf8_lossy(&buf)
            .trim_end_matches(['\r', '\n'])
            .to_owned())
    }

    /// One reply, which may span several lines (`123-first`, ..., `123 last`).
    async fn read_reply(&mut self) -> Result<Reply, EngineError> {
        timeout(REPLY_TIMEOUT, self.read_reply_inner())
            .await
            .map_err(|_| EngineError::Network("the server did not answer in time".into()))?
    }

    async fn read_reply_inner(&mut self) -> Result<Reply, EngineError> {
        let first = self.read_line().await?;
        let bad = || EngineError::Network(format!("not an FTP reply: {:?}", cut(&first)));
        let code: u16 = first
            .get(..3)
            .filter(|c| c.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|c| c.parse().ok())
            .ok_or_else(bad)?;
        let multi = match first.as_bytes().get(3) {
            Some(b'-') => true,
            Some(b' ') | None => false,
            Some(_) => return Err(bad()),
        };
        let mut text = first.get(4..).unwrap_or("").to_owned();
        if multi {
            let end = format!("{code} ");
            for _ in 0..MAX_REPLY_LINES {
                let line = self.read_line().await?;
                if let Some(rest) = line.strip_prefix(&end) {
                    text.push('\n');
                    text.push_str(rest);
                    return Ok(Reply { code, text });
                }
                text.push('\n');
                text.push_str(&line);
            }
            return Err(EngineError::Network("a reply is too long".into()));
        }
        Ok(Reply { code, text })
    }

    /// Send one command and read its reply.
    async fn command(&mut self, line: &str) -> Result<Reply, EngineError> {
        if has_control(line) {
            return Err(EngineError::InvalidUrl(
                "a name contains a control character".into(),
            ));
        }
        let mut out = String::with_capacity(line.len() + 2);
        out.push_str(line);
        out.push_str("\r\n");
        let io = self.ctrl.get_mut();
        timeout(REPLY_TIMEOUT, async {
            io.write_all(out.as_bytes()).await?;
            io.flush().await
        })
        .await
        .map_err(|_| EngineError::Network("the server did not accept the command".into()))?
        .map_err(|e| net_err("writing to the server failed", e))?;
        self.read_reply().await
    }

    async fn login(&mut self, login: Option<&Credentials>) -> Result<(), EngineError> {
        let (user, pass) = match login {
            Some(c) => (c.username.as_str(), c.password.as_str()),
            None => ("anonymous", "anonymous@"),
        };
        let r = self.command(&format!("USER {user}")).await?;
        match r.code {
            230 => return Ok(()),
            331 | 332 => {}
            _ => return Err(refused(&r)),
        }
        let r = self.command(&format!("PASS {pass}")).await?;
        match r.code {
            230 | 202 => Ok(()),
            _ => Err(refused(&r)),
        }
    }

    /// Learn what the server can do and switch to binary transfers.
    async fn negotiate(&mut self) -> Result<(), EngineError> {
        let feat = self.command("FEAT").await?;
        let mut utf8 = false;
        if feat.code == 211 {
            for line in feat.text.lines().skip(1) {
                let f = line.trim().to_ascii_uppercase();
                self.mlsd |= f.starts_with("MLST");
                self.rest |= f.starts_with("REST");
                utf8 |= f == "UTF8";
            }
        }
        if utf8 {
            let _ = self.command("OPTS UTF8 ON").await?;
        }
        if !self.rest {
            // A server without FEAT may still be able to continue.
            self.rest = self.command("REST 0").await?.code == 350;
        }
        self.command("TYPE I").await?.expect(&[200])?;
        Ok(())
    }

    /// Open the data connection (passive mode). `EPSV` names only a port; `PASV` also names an
    /// address, which is ignored: the data goes where the control connection goes.
    async fn passive(&mut self) -> Result<TcpStream, EngineError> {
        let r = self.command("EPSV").await?;
        let port = if r.code == 229 {
            parse_epsv(&r.text)
        } else {
            let r = self.command("PASV").await?.expect(&[227])?;
            parse_pasv(&r.text)
        }
        .ok_or_else(|| EngineError::Network("unreadable passive-mode reply".into()))?;
        timeout(net::CONNECT_TIMEOUT, TcpStream::connect((self.peer, port)))
            .await
            .map_err(|_| EngineError::Network("the data connection timed out".into()))?
            .map_err(|e| net_err("cannot open the data connection", e))
    }

    /// Protect the data connection when the control connection is protected.
    async fn secure_data(&self, tcp: TcpStream) -> Result<BoxIo, EngineError> {
        match &self.tls {
            Some((connector, name)) => handshake(connector, name, tcp).await,
            None => Ok(Box::new(tcp)),
        }
    }

    /// Size of a file, if the server tells.
    async fn size(&mut self, path: &str) -> Result<Option<u64>, EngineError> {
        let r = self.command(&format!("SIZE {path}")).await?;
        Ok((r.code == 213)
            .then(|| r.text.trim().parse().ok())
            .flatten())
    }

    /// When the file was last changed, as the server writes it (`20240131120000`).
    async fn modified(&mut self, path: &str) -> Result<Option<String>, EngineError> {
        let r = self.command(&format!("MDTM {path}")).await?;
        Ok((r.code == 213).then(|| r.text.trim().to_owned()))
    }

    async fn is_dir(&mut self, path: &str) -> Result<bool, EngineError> {
        Ok(self.command(&format!("CWD {path}")).await?.code == 250)
    }

    /// Start sending `path` from byte `offset`. Read the data from the returned stream.
    async fn retrieve(&mut self, path: &str, offset: u64) -> Result<BoxIo, EngineError> {
        let tcp = self.passive().await?;
        if offset > 0 {
            if !self.rest {
                return Err(EngineError::Ftp {
                    code: 502,
                    message: "the server cannot continue a download".into(),
                });
            }
            self.command(&format!("REST {offset}"))
                .await?
                .expect(&[350])?;
        }
        self.command(&format!("RETR {path}"))
            .await?
            .expect(&[125, 150])?;
        self.secure_data(tcp).await
    }

    /// The lines a folder listing command sends over the data connection.
    async fn listing_text(&mut self, command: &str) -> Result<String, EngineError> {
        let tcp = self.passive().await?;
        self.command(command).await?.expect(&[125, 150])?;
        let mut io = self.secure_data(tcp).await?;
        let mut data = Vec::new();
        loop {
            let mut buf = vec![0u8; CHUNK];
            let n = match timeout(IDLE_TIMEOUT, io.read(&mut buf)).await {
                Ok(Ok(n)) => n,
                // Some servers end a protected connection without the closing handshake.
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => 0,
                Ok(Err(e)) => return Err(net_err("reading the listing failed", e)),
                Err(_) => return Err(EngineError::Network("the listing stalled".into())),
            };
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
            if data.len() > MAX_LISTING_BYTES {
                return Err(EngineError::Network("the listing is too long".into()));
            }
        }
        let _ = timeout(Duration::from_secs(2), io.shutdown()).await;
        drop(io);
        self.read_reply().await?.expect(&[226, 250])?;
        Ok(String::from_utf8_lossy(&data).into_owned())
    }

    /// Say goodbye. Whatever happens, the connection is dropped after this.
    async fn quit(&mut self) {
        let _ = timeout(Duration::from_secs(3), async {
            let io = self.ctrl.get_mut();
            io.write_all(b"QUIT\r\n").await?;
            io.flush().await
        })
        .await;
    }
}

fn refused(r: &Reply) -> EngineError {
    match r.code {
        // Wrong password, unknown user, "login incorrect".
        530 | 532 | 331 | 430 | 534 | 535 => EngineError::Auth(r.text.clone()),
        _ => r.error(),
    }
}

/// A reply shortened for a message.
fn cut(s: &str) -> String {
    s.chars().take(80).collect()
}

/// `229 Entering Extended Passive Mode (|||6446|)` -> `6446`.
fn parse_epsv(text: &str) -> Option<u16> {
    let inner = text.split_once('(')?.1.split_once(')')?.0;
    let delim = inner.chars().next()?;
    inner.split(delim).nth(3)?.trim().parse().ok()
}

/// `227 Entering Passive Mode (192,168,1,2,19,137)` -> `19 * 256 + 137`.
fn parse_pasv(text: &str) -> Option<u16> {
    let inner = text.split_once('(')?.1.split_once(')')?.0;
    let n: Vec<u16> = inner
        .split(',')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    match n[..] {
        [_, _, _, _, hi @ 0..=255, lo @ 0..=255] => Some(hi * 256 + lo),
        _ => None,
    }
}

/// The path of an address as the FTP commands want it (RFC 1738): relative to where the login
/// starts, so `ftp://host/pub/a` is `pub/a`; a second slash (`ftp://host//pub/a`) is the root.
pub fn command_path(address: &Address) -> String {
    let path = address.path();
    path.strip_prefix('/').unwrap_or(&path).to_owned()
}

/// Look at an address: a file (size, date, whether it can be continued) or a folder (its list).
pub async fn inspect(
    policy: UrlPolicy,
    target: &Target,
    address: &Address,
) -> Result<Inspected, EngineError> {
    let mut s = connect(policy, target).await?;
    let result = inspect_inner(&mut s, address).await;
    s.quit().await;
    result
}

async fn inspect_inner(s: &mut Session, address: &Address) -> Result<Inspected, EngineError> {
    let path = command_path(address);
    let folder = path.is_empty() || path.ends_with('/');
    if !folder {
        if let Some(size) = s.size(&path).await? {
            let modified = s.modified(&path).await?;
            return Ok(Inspected::File {
                size: Some(size),
                modified,
                resumable: s.rest,
            });
        }
        // No size: a folder, or a server that cannot say. A folder is told by entering it.
        if !s.is_dir(&path).await? {
            // Maybe it is a file and the server only lacks SIZE: let the download find out.
            let modified = s.modified(&path).await?;
            if modified.is_some() {
                return Ok(Inspected::File {
                    size: None,
                    modified,
                    resumable: false,
                });
            }
            return Err(EngineError::NotFound(format!(
                "{path:?} is not on the server"
            )));
        }
    }
    let command = match (s.mlsd, path.is_empty()) {
        (true, true) => "MLSD".to_owned(),
        (true, false) => format!("MLSD {path}"),
        (false, true) => "LIST".to_owned(),
        (false, false) => format!("LIST {path}"),
    };
    let text = s.listing_text(&command).await?;
    let raw = if s.mlsd {
        parse_mlsd(&text)
    } else {
        parse_list(&text)
    };
    Ok(Inspected::Dir(crate::listing::from_raw(&address.url, raw)))
}

/// `type=file;size=1234;modify=20240101120000; the name` lines (RFC 3659).
fn parse_mlsd(text: &str) -> Vec<RawEntry> {
    text.lines()
        .filter_map(|line| {
            let (facts, name) = line.split_once(' ')?;
            let mut kind = "";
            let mut size = None;
            for fact in facts.split(';') {
                let Some((k, v)) = fact.split_once('=') else {
                    continue;
                };
                match k.to_ascii_lowercase().as_str() {
                    "type" => kind = v,
                    "size" => size = v.parse().ok(),
                    _ => {}
                }
            }
            let is_dir = match kind.to_ascii_lowercase().as_str() {
                "file" => false,
                "dir" => true,
                // cdir and pdir are the folder itself and its parent; OS-specific kinds are not files.
                _ => return None,
            };
            Some(RawEntry {
                name: name.to_owned(),
                size: if is_dir { None } else { size },
                is_dir,
            })
        })
        .collect()
}

/// The output of `LIST`: the Unix `ls -l` form (also used by most NAS boxes) and the DOS form of IIS.
fn parse_list(text: &str) -> Vec<RawEntry> {
    text.lines().filter_map(parse_list_line).collect()
}

fn parse_list_line(line: &str) -> Option<RawEntry> {
    let line = line.trim_end();
    let first = line.chars().next()?;
    if matches!(first, 'd' | '-' | 'l') && line.len() > 10 {
        // drwxr-xr-x 2 owner group 4096 Jan  1 12:00 name   (name = what follows 8 fields)
        let size: Option<u64> = nth_field(line, 4).and_then(|f| f.parse().ok());
        let name = rest_after_fields(line, 8)?;
        // A link shows as `name -> target`; the name is what can be asked for.
        let name = if first == 'l' {
            name.split_once(" -> ").map_or(name, |(n, _)| n)
        } else {
            name
        };
        return Some(RawEntry {
            name: name.to_owned(),
            size: if first == 'd' { None } else { size },
            is_dir: first == 'd',
        });
    }
    // 01-31-24  12:00PM       <DIR>          name   /   01-31-24  12:00PM   1234 name
    if first.is_ascii_digit() {
        let kind = nth_field(line, 2)?;
        let name = rest_after_fields(line, 3)?;
        return if kind.eq_ignore_ascii_case("<DIR>") {
            Some(RawEntry {
                name: name.to_owned(),
                size: None,
                is_dir: true,
            })
        } else {
            Some(RawEntry {
                name: name.to_owned(),
                size: kind.replace(',', "").parse().ok(),
                is_dir: false,
            })
        };
    }
    None
}

fn nth_field(line: &str, n: usize) -> Option<&str> {
    line.split_ascii_whitespace().nth(n)
}

/// What follows the first `n` whitespace-separated fields (spaces inside it are kept).
fn rest_after_fields(line: &str, n: usize) -> Option<&str> {
    let mut rest = line.trim_start();
    for _ in 0..n {
        let end = rest.find(char::is_whitespace)?;
        rest = rest[end..].trim_start();
    }
    (!rest.is_empty()).then_some(rest)
}

/// The data of a file on its way, as the download worker wants it.
pub struct Chunks {
    session: Session,
    io: Option<BoxIo>,
    buf: Vec<u8>,
}

/// Open `path` from `offset` for the download worker.
pub async fn open(
    policy: UrlPolicy,
    target: &Target,
    address: &Address,
    offset: u64,
) -> Result<Chunks, EngineError> {
    let mut session = connect(policy, target).await?;
    let path = command_path(address);
    let io = session.retrieve(&path, offset).await?;
    Ok(Chunks {
        session,
        io: Some(io),
        buf: vec![0; CHUNK],
    })
}

#[async_trait]
impl ChunkSource for Chunks {
    async fn next(&mut self) -> Result<Option<Bytes>, EngineError> {
        let Some(io) = self.io.as_mut() else {
            return Ok(None);
        };
        let n = match timeout(IDLE_TIMEOUT, io.read(&mut self.buf)).await {
            Ok(Ok(n)) => n,
            // A protected connection that is closed without the closing handshake: the length of
            // what arrived tells whether it was complete.
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => 0,
            Ok(Err(e)) => return Err(net_err("reading from the server failed", e)),
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

    async fn close(&mut self, complete: bool) {
        // End the data connection first (with TLS, properly): many servers send their final reply
        // only after they have seen it close.
        if let Some(mut io) = self.io.take() {
            let _ = timeout(Duration::from_secs(2), io.shutdown()).await;
        }
        if complete {
            // The server's last word on the transfer; a refusal here does not undo the bytes.
            let _ = timeout(Duration::from_secs(5), self.session.read_reply()).await;
        }
        self.session.quit().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_replies() {
        assert_eq!(
            parse_epsv("Entering Extended Passive Mode (|||6446|)"),
            Some(6446)
        );
        assert_eq!(parse_epsv("Extended Passive Mode (!!!2121!)"), Some(2121));
        assert_eq!(parse_epsv("no parens"), None);
        assert_eq!(
            parse_pasv("Entering Passive Mode (192,168,1,2,19,137)"),
            Some(19 * 256 + 137)
        );
        assert_eq!(parse_pasv("Entering Passive Mode (1,2,3,4,300,1)"), None);
        assert_eq!(parse_pasv("(1,2,3)"), None);
    }

    #[test]
    fn unix_listings() {
        let text = "total 12\r\n\
drwxr-xr-x   2 ftp      ftp          4096 Jan 01 12:00 sub dir\r\n\
-rw-r--r--   1 ftp      ftp       1048576 Jan  1  2023 big file.iso\r\n\
lrwxrwxrwx   1 ftp      ftp             9 Jan 01 12:00 latest -> a.iso\r\n\
-rw-r--r--   1 ftp      ftp             0 Feb 29 10:10 empty\r\n";
        let got = parse_list(text);
        let names: Vec<_> = got
            .iter()
            .map(|e| (e.name.as_str(), e.size, e.is_dir))
            .collect();
        assert_eq!(
            names,
            [
                ("sub dir", None, true),
                ("big file.iso", Some(1_048_576), false),
                ("latest", Some(9), false),
                ("empty", Some(0), false),
            ]
        );
    }

    #[test]
    fn dos_listings() {
        let text = "01-31-24  12:00PM       <DIR>          Docs and more\r\n\
02-01-24  09:15AM              1,234 notes.txt\r\n";
        let got = parse_list(text);
        assert_eq!(got.len(), 2);
        assert_eq!(
            (got[0].name.as_str(), got[0].is_dir),
            ("Docs and more", true)
        );
        assert_eq!(
            (got[1].name.as_str(), got[1].size, got[1].is_dir),
            ("notes.txt", Some(1234), false)
        );
    }

    #[test]
    fn machine_listings() {
        let text = "type=cdir;modify=20240101000000; .\r\n\
type=pdir; ..\r\n\
type=dir;modify=20240101000000;perm=el; photos\r\n\
type=file;size=1234;modify=20240101120000;perm=r; a b.zip\r\n\
type=OS.unix=slink:/x;size=5; link\r\n";
        let got = parse_mlsd(text);
        assert_eq!(
            got,
            [
                RawEntry {
                    name: "photos".into(),
                    size: None,
                    is_dir: true
                },
                RawEntry {
                    name: "a b.zip".into(),
                    size: Some(1234),
                    is_dir: false
                },
            ]
        );
    }

    #[test]
    fn paths_are_relative_to_the_login_unless_doubled() {
        use crate::source::parse;
        let p = |raw| command_path(&parse(raw).expect("url"));
        assert_eq!(p("ftp://h/pub/a%20b.iso"), "pub/a b.iso");
        assert_eq!(p("ftp://h//srv/ftp/a.iso"), "/srv/ftp/a.iso");
        assert_eq!(p("ftp://h/"), "");
        assert_eq!(p("ftp://h"), "");
        assert_eq!(p("ftp://h/pub/"), "pub/");
    }
}
