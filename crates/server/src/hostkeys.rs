//! The SSH host keys the user accepted, kept in memory for the engine (it asks at every
//! connection) next to the database that remembers them (SPEC 6.12).

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use fetchbox_core::KnownHost;
use fetchbox_http_engine::HostKeys;

/// `host:port` -> the accepted fingerprint.
#[derive(Default)]
pub struct HostKeyCache {
    keys: RwLock<HashMap<(String, u16), String>>,
}

impl HostKeyCache {
    /// Start from what the database has.
    pub fn load(hosts: &[KnownHost]) -> Self {
        let cache = Self::default();
        for h in hosts {
            cache.set(&h.host, h.port, &h.fingerprint);
        }
        cache
    }

    pub fn set(&self, host: &str, port: u16, fingerprint: &str) {
        self.keys
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert((host.to_ascii_lowercase(), port), fingerprint.to_owned());
    }

    pub fn forget(&self, host: &str, port: u16) {
        self.keys
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&(host.to_ascii_lowercase(), port));
    }
}

#[async_trait]
impl HostKeys for HostKeyCache {
    async fn trusted(&self, host: &str, port: u16) -> Option<String> {
        self.keys
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(host.to_ascii_lowercase(), port))
            .cloned()
    }
}
