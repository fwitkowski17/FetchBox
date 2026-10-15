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
            EngineError::Http(_) | EngineError::Truncated => true,
            EngineError::Status(s) => *s >= 500 || *s == 429,
            _ => false,
        }
    }
}

impl From<EngineError> for fetchbox_core::Error {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::InvalidUrl(m) => fetchbox_core::Error::InvalidInput(m),
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
