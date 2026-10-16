//! Engine-level errors.

/// Everything that can go wrong while probing or downloading.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("http error: {}", chain(.0))]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("server returned status {0}")]
    Status(u16),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("{}", fetchbox_core::error::no_space_message(*.needed, *.free, *.reserved))]
    NoSpace {
        needed: u64,
        free: u64,
        /// Part of `free` that the other running downloads still have to write.
        reserved: u64,
    },
    #[error("connection closed before the full body arrived")]
    Truncated,
    /// A reply of an FTP server that is not a success.
    #[error("ftp error {code}: {message}")]
    Ftp { code: u16, message: String },
    /// A connection to an FTP or SSH server failed or broke (not the disk, not HTTP).
    #[error("{0}")]
    Network(String),
    /// The server refused the login.
    #[error("login refused: {0}")]
    Auth(String),
    /// The SSH server's host key is not trusted, or is not the one that was accepted.
    #[error("{0}")]
    HostKey(Box<fetchbox_core::HostKeyProblem>),
    /// The file or folder asked for is not there, or may not be read.
    #[error("{0}")]
    NotFound(String),
}

/// `reqwest` hides the real reason (refused, DNS, blocked address) in the error source.
fn chain(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        out.push_str(": ");
        out.push_str(&s.to_string());
        source = s.source();
    }
    out
}

impl EngineError {
    /// Worth another attempt (network hiccup, overloaded server).
    pub fn is_retryable(&self) -> bool {
        match self {
            EngineError::Http(_) | EngineError::Truncated | EngineError::Network(_) => true,
            EngineError::Status(s) => *s >= 500 || *s == 429,
            // 4xx replies of an FTP server mean "try again later" (421: too many connections).
            EngineError::Ftp { code, .. } => (400..500).contains(code),
            _ => false,
        }
    }
}

impl From<EngineError> for fetchbox_core::Error {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::InvalidUrl(m) => fetchbox_core::Error::InvalidInput(m),
            EngineError::HostKey(problem) => fetchbox_core::Error::HostKey(problem),
            EngineError::NoSpace {
                needed,
                free,
                reserved,
            } => fetchbox_core::Error::InsufficientSpace {
                needed,
                free,
                reserved,
            },
            other => fetchbox_core::Error::Engine(other.to_string()),
        }
    }
}
