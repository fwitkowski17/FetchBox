//! A small FTP server for engine tests: users, passive mode, `REST`, listings in two styles, TLS
//! (explicit and implicit) and faults (a dropped transfer, a refused first connection). It records
//! every command it receives, so tests can also check what was *not* sent.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
type BoxIo = Box<dyn Io>;

/// How the server protects its connections.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Plain,
    Explicit,
    Implicit,
}

#[derive(Clone)]
pub struct Options {
    /// Users and passwords. Empty: only `anonymous` may log in.
    pub users: HashMap<String, String>,
    pub feat: bool,
    pub mlsd: bool,
    pub rest: bool,
    pub size: bool,
    pub epsv: bool,
    /// What `PASV` claims as the address (a lie, to see that it is ignored).
    pub pasv_ip: Option<String>,
    /// `LIST` in the DOS style instead of the Unix one.
    pub dos_list: bool,
    pub security: Security,
    /// Refuse these connections (counted from 0) with `421`.
    pub refuse: Vec<usize>,
    /// The certificate is one that clients are told to trust (tests of a bad one turn it off).
    pub trusted_cert: bool,
    /// Cut the first transfers off after this many bytes (one entry per transfer).
    pub cut_after: Vec<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            users: HashMap::new(),
            feat: true,
            mlsd: true,
            rest: true,
            size: true,
            epsv: true,
            pasv_ip: None,
            dos_list: false,
            security: Security::Plain,
            refuse: Vec::new(),
            trusted_cert: true,
            cut_after: Vec::new(),
        }
    }
}

struct State {
    files: BTreeMap<String, Vec<u8>>,
    options: Options,
    log: Mutex<Vec<String>>,
    cuts: Mutex<Vec<usize>>,
    refused: AtomicUsize,
    connections: AtomicUsize,
    acceptor: Option<TlsAcceptor>,
}

pub struct FtpServer {
    pub addr: SocketAddr,
    state: Arc<State>,
}

/// A certificate for `127.0.0.1`, and the same as the root a client must trust to accept it.
pub fn test_certificate() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let params = rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned()])
        .expect("certificate parameters");
    let key = rcgen::KeyPair::generate().expect("key");
    let cert = params.self_signed(&key).expect("certificate");
    (
        cert.der().clone(),
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
    )
}

impl FtpServer {
    /// Start a server holding `files` (paths like `pub/a.bin`, relative to the login folder).
    pub async fn spawn(files: &[(&str, Vec<u8>)], options: Options) -> Self {
        let acceptor = (options.security != Security::Plain).then(|| {
            let (cert, key) = test_certificate();
            // The client has to trust it: tests register the certificate as a root.
            if options.trusted_cert {
                crate::ftp::trust_for_tests(cert.clone());
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("versions")
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)
                .expect("server certificate");
            TlsAcceptor::from(Arc::new(config))
        });
        let state = Arc::new(State {
            files: files
                .iter()
                .map(|(p, d)| ((*p).to_owned(), d.clone()))
                .collect(),
            cuts: Mutex::new(options.cut_after.clone()),
            options,
            log: Mutex::new(Vec::new()),
            refused: AtomicUsize::new(0),
            connections: AtomicUsize::new(0),
            acceptor,
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let shared = Arc::clone(&state);
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    return;
                };
                let state = Arc::clone(&shared);
                tokio::spawn(async move {
                    let _ = session(sock, state).await;
                });
            }
        });
        Self { addr, state }
    }

    pub fn url(&self, scheme: &str, path: &str) -> String {
        format!("{scheme}://127.0.0.1:{}/{path}", self.addr.port())
    }

    /// Every command received so far (passwords are not recorded).
    pub fn commands(&self) -> Vec<String> {
        self.state.log.lock().expect("log").clone()
    }

    pub fn connections(&self) -> usize {
        self.state.connections.load(Ordering::SeqCst)
    }
}

async fn reply(io: &mut BufReader<BoxIo>, text: &str) -> std::io::Result<()> {
    io.get_mut()
        .write_all(format!("{text}\r\n").as_bytes())
        .await?;
    io.get_mut().flush().await
}

async fn session(sock: TcpStream, state: Arc<State>) -> std::io::Result<()> {
    state.connections.fetch_add(1, Ordering::SeqCst);
    let opts = &state.options;
    let io: BoxIo = match (&state.acceptor, opts.security) {
        (Some(acceptor), Security::Implicit) => Box::new(acceptor.accept(sock).await?),
        _ => Box::new(sock),
    };
    let mut io = BufReader::new(io);
    if opts
        .refuse
        .contains(&state.refused.fetch_add(1, Ordering::SeqCst))
    {
        return reply(&mut io, "421 Too many connections").await;
    }
    reply(&mut io, "220 test server").await?;

    let mut user = String::new();
    let mut logged_in = false;
    let mut rest = 0usize;
    let mut passive: Option<TcpListener> = None;
    let mut protected = opts.security == Security::Implicit;
    let mut line = String::new();
    loop {
        line.clear();
        if io.read_line(&mut line).await? == 0 {
            return Ok(());
        }
        let text = line.trim_end().to_owned();
        let (cmd, arg) = text.split_once(' ').unwrap_or((text.as_str(), ""));
        let cmd = cmd.to_ascii_uppercase();
        state.log.lock().expect("log").push(if cmd == "PASS" {
            "PASS ***".to_owned()
        } else {
            text.clone()
        });
        match cmd.as_str() {
            "AUTH" if opts.security == Security::Explicit => {
                reply(&mut io, "234 go ahead").await?;
                let Some(acceptor) = &state.acceptor else {
                    return Ok(());
                };
                let plain = io.into_inner();
                io = BufReader::new(Box::new(acceptor.accept(plain).await?));
                protected = true;
            }
            "AUTH" => reply(&mut io, "502 no TLS here").await?,
            "PBSZ" => reply(&mut io, "200 ok").await?,
            "PROT" => {
                protected = protected && arg.eq_ignore_ascii_case("P");
                reply(&mut io, "200 ok").await?
            }
            "USER" => {
                user = arg.to_owned();
                if user == "anonymous" && opts.users.is_empty() {
                    reply(&mut io, "230 welcome, guest").await?;
                    logged_in = true;
                } else {
                    reply(&mut io, "331 password please").await?;
                }
            }
            "PASS" => {
                if opts.users.get(&user).is_some_and(|p| p == arg) {
                    logged_in = true;
                    reply(&mut io, "230 welcome").await?;
                } else {
                    reply(&mut io, "530 Login incorrect.").await?;
                }
            }
            _ if !logged_in => reply(&mut io, "530 log in first").await?,
            "FEAT" if opts.feat => {
                let mut f = String::from("211-Features:\r\n UTF8\r\n");
                if opts.rest {
                    f.push_str(" REST STREAM\r\n");
                }
                if opts.mlsd {
                    f.push_str(" MLST type*;size*;modify*;\r\n");
                }
                f.push_str(" SIZE\r\n MDTM\r\n211 End");
                reply(&mut io, &f).await?
            }
            "FEAT" => reply(&mut io, "500 unknown command").await?,
            "OPTS" => reply(&mut io, "200 ok").await?,
            "TYPE" => reply(&mut io, "200 binary").await?,
            "PWD" => reply(&mut io, "257 \"/\"").await?,
            "CWD" => {
                if is_dir(&state, arg) {
                    reply(&mut io, "250 ok").await?
                } else {
                    reply(&mut io, "550 No such directory").await?
                }
            }
            "SIZE" if opts.size => match state.files.get(arg) {
                Some(d) => reply(&mut io, &format!("213 {}", d.len())).await?,
                None => reply(&mut io, "550 Could not get file size.").await?,
            },
            "SIZE" => reply(&mut io, "502 not implemented").await?,
            "MDTM" => match state.files.get(arg) {
                Some(_) => reply(&mut io, "213 20240131120000").await?,
                None => reply(&mut io, "550 No such file").await?,
            },
            "REST" if opts.rest => {
                rest = arg.parse().unwrap_or(0);
                reply(&mut io, "350 restarting").await?
            }
            "REST" => reply(&mut io, "502 not implemented").await?,
            "EPSV" if opts.epsv => {
                let l = TcpListener::bind("127.0.0.1:0").await?;
                let port = l.local_addr()?.port();
                passive = Some(l);
                reply(
                    &mut io,
                    &format!("229 Entering Extended Passive Mode (|||{port}|)"),
                )
                .await?
            }
            "EPSV" => reply(&mut io, "500 unknown command").await?,
            "PASV" => {
                let l = TcpListener::bind("127.0.0.1:0").await?;
                let port = l.local_addr()?.port();
                passive = Some(l);
                let ip = opts.pasv_ip.clone().unwrap_or_else(|| "127.0.0.1".into());
                let ip = ip.replace('.', ",");
                reply(
                    &mut io,
                    &format!(
                        "227 Entering Passive Mode ({ip},{},{})",
                        port / 256,
                        port % 256
                    ),
                )
                .await?
            }
            "RETR" => {
                let Some(data) = state.files.get(arg) else {
                    reply(&mut io, "550 No such file").await?;
                    continue;
                };
                let Some(l) = passive.take() else {
                    reply(&mut io, "425 use PASV first").await?;
                    continue;
                };
                reply(&mut io, "150 opening").await?;
                let (sock, _) = l.accept().await?;
                let mut out = data_io(&state, sock, protected).await?;
                let start = rest.min(data.len());
                rest = 0;
                let cut = {
                    let mut cuts = state.cuts.lock().expect("cuts");
                    (!cuts.is_empty()).then(|| cuts.remove(0))
                };
                let body = &data[start..];
                match cut {
                    Some(n) if n < body.len() => {
                        let _ = out.write_all(&body[..n]).await;
                        let _ = out.flush().await;
                        drop(out);
                        reply(&mut io, "426 Connection closed; transfer aborted.").await?;
                    }
                    _ => {
                        out.write_all(body).await?;
                        out.flush().await?;
                        let _ = out.shutdown().await;
                        drop(out);
                        reply(&mut io, "226 Transfer complete.").await?;
                    }
                }
            }
            "LIST" | "MLSD" => {
                if cmd == "MLSD" && !opts.mlsd {
                    reply(&mut io, "500 unknown command").await?;
                    continue;
                }
                let Some(l) = passive.take() else {
                    reply(&mut io, "425 use PASV first").await?;
                    continue;
                };
                if !is_dir(&state, arg) {
                    reply(&mut io, "550 No such directory").await?;
                    continue;
                }
                reply(&mut io, "150 listing").await?;
                let (sock, _) = l.accept().await?;
                let mut out = data_io(&state, sock, protected).await?;
                out.write_all(listing(&state, arg, cmd == "MLSD").as_bytes())
                    .await?;
                out.flush().await?;
                let _ = out.shutdown().await;
                drop(out);
                reply(&mut io, "226 done").await?;
            }
            "QUIT" => {
                reply(&mut io, "221 bye").await?;
                return Ok(());
            }
            _ => reply(&mut io, "502 not implemented").await?,
        }
    }
}

async fn data_io(state: &State, sock: TcpStream, protected: bool) -> std::io::Result<BoxIo> {
    match (&state.acceptor, protected) {
        (Some(acceptor), true) => Ok(Box::new(acceptor.accept(sock).await?)),
        _ => Ok(Box::new(sock)),
    }
}

fn dir_prefix(path: &str) -> String {
    let p = path.trim_matches('/');
    if p.is_empty() {
        String::new()
    } else {
        format!("{p}/")
    }
}

fn is_dir(state: &State, path: &str) -> bool {
    let prefix = dir_prefix(path);
    prefix.is_empty() || state.files.keys().any(|k| k.starts_with(&prefix))
}

/// The direct children of a folder: (name, is_dir, size).
fn children(state: &State, path: &str) -> Vec<(String, bool, usize)> {
    let prefix = dir_prefix(path);
    let mut out: BTreeMap<String, (bool, usize)> = BTreeMap::new();
    for (k, v) in &state.files {
        let Some(rest) = k.strip_prefix(&prefix) else {
            continue;
        };
        match rest.split_once('/') {
            Some((dir, _)) => {
                out.insert(dir.to_owned(), (true, 0));
            }
            None => {
                out.insert(rest.to_owned(), (false, v.len()));
            }
        }
    }
    out.into_iter().map(|(n, (d, s))| (n, d, s)).collect()
}

fn listing(state: &State, path: &str, machine: bool) -> String {
    let mut out = String::new();
    if machine {
        out.push_str("type=cdir; .\r\ntype=pdir; ..\r\n");
    }
    for (name, dir, size) in children(state, path) {
        if machine {
            let kind = if dir { "dir" } else { "file" };
            out.push_str(&format!(
                "type={kind};size={size};modify=20240101000000; {name}\r\n"
            ));
        } else if state.options.dos_list {
            if dir {
                out.push_str(&format!(
                    "01-31-24  12:00PM       <DIR>          {name}\r\n"
                ));
            } else {
                out.push_str(&format!("01-31-24  12:00PM {size:>20} {name}\r\n"));
            }
        } else if dir {
            out.push_str(&format!(
                "drwxr-xr-x   2 ftp ftp 4096 Jan 01 12:00 {name}\r\n"
            ));
        } else {
            out.push_str(&format!(
                "-rw-r--r--   1 ftp ftp {size} Jan 01 12:00 {name}\r\n"
            ));
        }
    }
    out
}
