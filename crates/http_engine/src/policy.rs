//! Optional blocklist of private address ranges (SPEC 6.7, SSRF).
//!
//! Off by default: fetching from a NAS or another box in the LAN is a normal use of a
//! homelab download manager. Turn it on when the manager is reachable from outside, so a
//! stranger cannot make it fetch `http://169.254.169.254/` or probe the internal network.
//!
//! Three places must agree or the policy can be bypassed:
//! - IP literals in the URL are checked up front ([`UrlPolicy::check`]);
//! - host names are filtered where they are resolved ([`FilteringResolver`]), which also
//!   closes DNS-rebinding tricks that resolve differently later;
//! - every redirect hop is checked again (a public URL may redirect to a private one).

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use reqwest::Url;

use crate::error::EngineError;

const MAX_REDIRECTS: usize = 10;

/// Which destinations the engine may connect to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UrlPolicy {
    /// Refuse loopback, private, link-local and other non-public addresses.
    pub block_private: bool,
}

impl UrlPolicy {
    /// Reject URLs whose host is a blocked IP literal. Host names pass here and are
    /// filtered at resolution time.
    pub fn check(&self, url: &Url) -> Result<(), EngineError> {
        if !self.block_private {
            return Ok(());
        }
        let Some(host) = url.host_str() else {
            return Ok(());
        };
        let host = host.trim_start_matches('[').trim_end_matches(']');
        match host.parse::<IpAddr>() {
            Ok(ip) if is_private_ip(ip) => Err(EngineError::InvalidUrl(
                "address is in a private or local range, which is blocked".into(),
            )),
            _ => Ok(()),
        }
    }

    /// Redirect policy that applies this policy to every hop.
    pub(crate) fn redirect_policy(self) -> Policy {
        Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if self.check(attempt.url()).is_err() {
                attempt.error("redirect to a blocked address")
            } else {
                attempt.follow()
            }
        })
    }

    /// DNS resolver that drops blocked addresses, or `None` when nothing is blocked.
    pub(crate) fn resolver(self) -> Option<Arc<FilteringResolver>> {
        self.block_private.then(|| Arc::new(FilteringResolver))
    }
}

/// Loopback, private, link-local (incl. cloud metadata), CGNAT, multicast, unspecified,
/// broadcast, IPv6 unique-local; IPv4-mapped IPv6 addresses are judged by their IPv4 part.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                // 100.64.0.0/10, carrier-grade NAT
                || (o[0] == 100 && (o[1] & 0xC0) == 0x40)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_ip(IpAddr::V4(v4));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
    }
}

pub(crate) struct FilteringResolver;

impl Resolve for FilteringResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_owned();
            let found = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let allowed: Vec<SocketAddr> = found.filter(|a| !is_private_ip(a.ip())).collect();
            if allowed.is_empty() {
                return Err(
                    format!("{host} resolves only to blocked (private/local) addresses").into(),
                );
            }
            Ok(Box::new(allowed.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("ip")
    }

    #[test]
    fn classifies_addresses() {
        for blocked in [
            "127.0.0.1",
            "127.8.9.10",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:169.254.169.254",
        ] {
            assert!(is_private_ip(ip(blocked)), "{blocked} should be blocked");
        }
        for open in [
            "8.8.8.8",
            "1.1.1.1",
            "172.15.255.255",
            "172.32.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "93.184.216.34",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
        ] {
            assert!(!is_private_ip(ip(open)), "{open} should be allowed");
        }
    }

    #[test]
    fn literal_hosts_are_checked_only_when_enabled() {
        let on = UrlPolicy {
            block_private: true,
        };
        let off = UrlPolicy::default();
        for u in [
            "http://127.0.0.1/a",
            "http://192.168.0.5:8080/a",
            "http://[::1]/a",
            "http://169.254.169.254/latest/meta-data",
            "http://[::ffff:10.0.0.1]/a",
            // The URL parser normalizes tricky IPv4 spellings to dotted form.
            "http://2130706433/a",
            "http://0x7f.1/a",
            "http://127.1/a",
        ] {
            let url = Url::parse(u).expect("url");
            assert!(on.check(&url).is_err(), "{u} should be refused");
            assert!(off.check(&url).is_ok(), "{u} is fine without the policy");
        }
        for u in [
            "http://8.8.8.8/a",
            "https://example.com/a",
            "http://[2001:4860:4860::8888]/",
        ] {
            assert!(on.check(&Url::parse(u).expect("url")).is_ok(), "{u}");
        }
    }

    #[test]
    fn resolver_only_exists_when_blocking() {
        assert!(UrlPolicy::default().resolver().is_none());
        assert!(UrlPolicy {
            block_private: true
        }
        .resolver()
        .is_some());
    }

    #[tokio::test]
    async fn resolver_refuses_names_that_resolve_to_local_addresses() {
        let resolver = UrlPolicy {
            block_private: true,
        }
        .resolver()
        .expect("resolver");
        let name: Name = "localhost".parse().expect("name");
        assert!(resolver.resolve(name).await.is_err());
    }
}
