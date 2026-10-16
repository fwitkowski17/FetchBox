//! A small SFTP server for engine tests, built from the same two libraries as the client: a host
//! key it makes up, password logins, files and folders held in memory, and a transfer that can be
//! cut off.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::{Algorithm, HashAlg, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
pub struct Options {
    /// User names and passwords.
    pub users: HashMap<String, String>,
    /// Cut the first transfers off after this many bytes (one entry per opened file).
    pub cut_after: Vec<usize>,
    /// Wait this many milliseconds before answering a read (0: at once), so that a test has time
    /// to pause a transfer.
    pub read_delay_ms: u64,
}

struct State {
    /// Absolute paths (`/srv/a.bin`) and their contents.
    files: BTreeMap<String, Vec<u8>>,
    options: Options,
    cuts: Mutex<Vec<usize>>,
    logins: Mutex<usize>,
}

pub struct SftpServer {
    pub addr: SocketAddr,
    /// `SHA256:...`, as the client shows it.
    pub fingerprint: String,
    state: Arc<State>,
}

impl SftpServer {
    pub async fn spawn(files: &[(&str, Vec<u8>)], options: Options) -> Self {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host key");
        let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
        let config = russh::server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..russh::server::Config::default()
        };
        let state = Arc::new(State {
            files: files
                .iter()
                .map(|(p, d)| ((*p).to_owned(), d.clone()))
                .collect(),
            cuts: Mutex::new(options.cut_after.clone()),
            options,
            logins: Mutex::new(0),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let shared = Arc::clone(&state);
        tokio::spawn(async move {
            let mut server = Server { state: shared };
            let _ = server.run_on_socket(Arc::new(config), &listener).await;
        });
        Self {
            addr,
            fingerprint,
            state,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("sftp://127.0.0.1:{}/{path}", self.addr.port())
    }

    /// How many logins were accepted.
    pub fn logins(&self) -> usize {
        *self.state.logins.lock().expect("logins")
    }
}

#[derive(Clone)]
struct Server {
    state: Arc<State>,
}

impl russh::server::Server for Server {
    type Handler = Ssh;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Ssh {
        Ssh {
            state: Arc::clone(&self.state),
            channels: HashMap::new(),
        }
    }
}

struct Ssh {
    state: Arc<State>,
    channels: HashMap<ChannelId, Channel<Msg>>,
}

impl russh::server::Handler for Ssh {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if self
            .state
            .options
            .users
            .get(user)
            .is_some_and(|p| p == password)
        {
            *self.state.logins.lock().expect("logins") += 1;
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        match (name, self.channels.remove(&channel_id)) {
            ("sftp", Some(channel)) => {
                session.channel_success(channel_id)?;
                let sftp = Sftp {
                    state: Arc::clone(&self.state),
                    cut: None,
                    served: 0,
                    listed: HashSet::new(),
                };
                russh_sftp::server::run(channel.into_stream(), sftp).await;
            }
            _ => session.channel_failure(channel_id)?,
        }
        Ok(())
    }
}

struct Sftp {
    state: Arc<State>,
    /// Where this connection's transfer is cut off, if it is.
    cut: Option<usize>,
    served: usize,
    /// Folders whose list has been sent.
    listed: HashSet<String>,
}

fn attrs(size: Option<u64>, dir: bool) -> FileAttributes {
    FileAttributes {
        size,
        permissions: Some(if dir { 0o040_755 } else { 0o100_644 }),
        mtime: Some(1_700_000_000),
        ..FileAttributes::empty()
    }
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".into(),
        language_tag: "en".into(),
    }
}

/// `/a/b/` -> `/a/b/`, `/a/b` -> `/a/b/`, `/` -> `/`.
fn dir_prefix(path: &str) -> String {
    let p = path.trim_end_matches('/');
    format!("{p}/")
}

/// What a relative path means: it starts in the home folder.
fn absolute(path: &str) -> String {
    match path {
        "." => "/home/user".to_owned(),
        p if p.starts_with('/') => p.to_owned(),
        p => format!("/home/user/{p}"),
    }
}

impl Sftp {
    fn is_dir(&self, path: &str) -> bool {
        let prefix = dir_prefix(path);
        self.state.files.keys().any(|k| k.starts_with(&prefix))
    }
}

impl russh_sftp::server::Handler for Sftp {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        _version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        Ok(Name {
            id,
            files: vec![File::dummy(absolute(&path))],
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let path = absolute(&path);
        if let Some(data) = self.state.files.get(&path) {
            return Ok(Attrs {
                id,
                attrs: attrs(Some(data.len() as u64), false),
            });
        }
        if self.is_dir(&path) {
            return Ok(Attrs {
                id,
                attrs: attrs(None, true),
            });
        }
        Err(StatusCode::NoSuchFile)
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.stat(id, path).await
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        self.stat(id, handle).await
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        _flags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let filename = absolute(&filename);
        if !self.state.files.contains_key(&filename) {
            return Err(StatusCode::NoSuchFile);
        }
        let mut cuts = self.state.cuts.lock().expect("cuts");
        if !cuts.is_empty() {
            self.cut = Some(cuts.remove(0));
        }
        Ok(Handle {
            id,
            handle: filename,
        })
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        if self.state.options.read_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.state.options.read_delay_ms)).await;
        }
        let data = self
            .state
            .files
            .get(&handle)
            .ok_or(StatusCode::NoSuchFile)?;
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start >= data.len() {
            return Err(StatusCode::Eof);
        }
        // A server answers at most this much to one request, whatever was asked.
        let end = data.len().min(start + (len as usize).min(32 * 1024));
        let chunk = data[start..end].to_vec();
        self.served += chunk.len();
        if self.cut.is_some_and(|cut| self.served > cut) {
            return Err(StatusCode::ConnectionLost);
        }
        Ok(Data { id, data: chunk })
    }

    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
        Ok(ok(id))
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let path = absolute(&path);
        if !self.is_dir(&path) {
            return Err(StatusCode::NoSuchFile);
        }
        Ok(Handle { id, handle: path })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        if !self.listed.insert(handle.clone()) {
            return Err(StatusCode::Eof);
        }
        let prefix = dir_prefix(&handle);
        let mut entries: BTreeMap<String, FileAttributes> = BTreeMap::new();
        for (path, data) in &self.state.files {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((dir, _)) => {
                    entries.insert(dir.to_owned(), attrs(None, true));
                }
                None => {
                    entries.insert(rest.to_owned(), attrs(Some(data.len() as u64), false));
                }
            }
        }
        let mut files = vec![
            File::new(".", attrs(None, true)),
            File::new("..", attrs(None, true)),
        ];
        files.extend(entries.into_iter().map(|(n, a)| File::new(n, a)));
        Ok(Name { id, files })
    }
}
