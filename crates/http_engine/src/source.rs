//! Where a download comes from: the scheme of an address decides which protocol is spoken
//! (SPEC 6.12), and a login written into the address is taken out of it.

use fetchbox_core::Credentials;
use reqwest::Url;

use crate::error::EngineError;
use crate::listing::Listing;
use crate::probe::percent_decode;

/// The protocols the engine speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// `http://` and `https://`.
    Http,
    /// `ftp://`: plain FTP.
    Ftp,
    /// `ftps://`: FTP over TLS, explicit (`AUTH TLS` on the usual port) or implicit (port 990).
    Ftps,
    /// `sftp://`: SSH file transfer.
    Sftp,
}

/// A usable address.
#[derive(Debug, Clone)]
pub struct Address {
    pub url: Url,
    pub scheme: Scheme,
}

impl Address {
    /// The port, the protocol's own when the address names none.
    pub fn port(&self) -> u16 {
        self.url.port().unwrap_or(match self.scheme {
            Scheme::Http => match self.url.scheme() {
                "https" => 443,
                _ => 80,
            },
            Scheme::Ftp | Scheme::Ftps => 21,
            Scheme::Sftp => 22,
        })
    }

    /// The host name or address, lowercase and without the brackets of an IPv6 literal.
    pub fn host(&self) -> String {
        self.url
            .host_str()
            .unwrap_or_default()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase()
    }

    /// The path of the address with its escapes undone.
    pub fn path(&self) -> String {
        percent_decode(self.url.path())
    }
}

/// What a server answered about an address (FTP and SFTP): a file, or a folder and what is in it.
pub enum Inspected {
    File {
        size: Option<u64>,
        /// When it was last changed, in whatever form the server gave (compared, never shown).
        modified: Option<String>,
        /// It can be continued from an offset.
        resumable: bool,
    },
    Dir(Listing),
}

/// Check an address and find out which protocol it asks for.
pub fn parse(raw: &str) -> Result<Address, EngineError> {
    let url = Url::parse(raw.trim()).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    let scheme = match url.scheme() {
        "http" | "https" => Scheme::Http,
        "ftp" => Scheme::Ftp,
        "ftps" => Scheme::Ftps,
        "sftp" => Scheme::Sftp,
        other => {
            return Err(EngineError::InvalidUrl(format!(
                "unsupported scheme {other:?} (use http, https, ftp, ftps or sftp)"
            )))
        }
    };
    if url.host_str().is_none_or(str::is_empty) {
        return Err(EngineError::InvalidUrl("missing host".into()));
    }
    let address = Address { url, scheme };
    // A command of the FTP protocol is a line: a new line in a name would be a second command.
    if scheme != Scheme::Http && has_control(&address.path()) {
        return Err(EngineError::InvalidUrl(
            "the path contains a control character".into(),
        ));
    }
    Ok(address)
}

/// The login written into an address (`ftp://user:password@host/file`), taken out of it: the
/// address as it may be stored and shown, and the login to keep apart. `explicit` wins when both
/// are given. The user name and the password are unescaped.
pub fn split_login(
    raw: &str,
    explicit: Option<&Credentials>,
) -> Result<(String, Option<Credentials>), EngineError> {
    let mut url = Url::parse(raw.trim()).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    let from_url = (!url.username().is_empty() || url.password().is_some()).then(|| Credentials {
        username: percent_decode(url.username()),
        password: percent_decode(url.password().unwrap_or("")),
    });
    if from_url.is_some() {
        // Not storable if they cannot be taken out: refuse rather than keep a password in a URL.
        url.set_username("")
            .and_then(|()| url.set_password(None))
            .map_err(|()| {
                EngineError::InvalidUrl("cannot remove the login from the address".into())
            })?;
    }
    let login = explicit.cloned().or(from_url);
    if let Some(c) = &login {
        if has_control(&c.username) || has_control(&c.password) {
            return Err(EngineError::InvalidUrl(
                "the login contains a control character".into(),
            ));
        }
    }
    Ok((url.to_string(), login))
}

/// A character that must never reach a protocol line.
pub(crate) fn has_control(s: &str) -> bool {
    s.chars().any(|c| c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_and_default_ports() {
        let cases = [
            ("http://h/a", Scheme::Http, 80),
            ("https://h/a", Scheme::Http, 443),
            ("ftp://h/a", Scheme::Ftp, 21),
            ("ftps://h/a", Scheme::Ftps, 21),
            ("ftps://h:990/a", Scheme::Ftps, 990),
            ("sftp://h/a", Scheme::Sftp, 22),
            ("SFTP://h:2222/a", Scheme::Sftp, 2222),
        ];
        for (raw, scheme, port) in cases {
            let a = parse(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
            assert_eq!((a.scheme, a.port()), (scheme, port), "{raw}");
        }
        for bad in [
            "file:///etc/passwd",
            "gopher://h/",
            "javascript:alert(1)",
            "ftp://",
            "sftp://:22/x",
            "not a url",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_new_line_in_the_path_is_refused() {
        // %0D%0A in an FTP path would end the command and start another one.
        assert!(parse("ftp://h/a%0D%0ADELE%20x").is_err());
        assert!(parse("sftp://h/a%00b").is_err());
        // HTTP escapes it itself.
        assert!(parse("http://h/a%0D%0Ab").is_ok());
        assert!(parse("ftp://h/a%20b").is_ok());
    }

    #[test]
    fn host_and_path_are_normalised() {
        let a = parse("ftp://NAS.Lan/My%20Files/a%C5%82.iso").expect("url");
        assert_eq!(a.host(), "nas.lan");
        assert_eq!(a.path(), "/My Files/ał.iso");
        let a = parse("sftp://[::1]:2222/x").expect("url");
        assert_eq!(a.host(), "::1");
    }

    #[test]
    fn the_login_is_taken_out_of_the_address() {
        let (url, login) =
            split_login("ftp://filip:p%40ss%3Aword@nas.lan:2121/pub/a.iso", None).expect("split");
        assert_eq!(url, "ftp://nas.lan:2121/pub/a.iso");
        let login = login.expect("login");
        assert_eq!(login.username, "filip");
        assert_eq!(login.password, "p@ss:word");

        // A user name without a password.
        let (url, login) = split_login("sftp://me@h/x", None).expect("split");
        assert_eq!(url, "sftp://h/x");
        assert_eq!(login.expect("login").password, "");

        // Nothing to take out.
        let (url, login) = split_login(" https://h/a?b=1 ", None).expect("split");
        assert_eq!(url, "https://h/a?b=1");
        assert!(login.is_none());

        // An explicit login wins, and the one in the address is still removed.
        let explicit = Credentials {
            username: "other".into(),
            password: "pw".into(),
        };
        let (url, login) = split_login("ftp://u:p@h/f", Some(&explicit)).expect("split");
        assert_eq!(url, "ftp://h/f");
        assert_eq!(login, Some(explicit));

        let bad = Credentials {
            username: "a\r\nb".into(),
            password: String::new(),
        };
        assert!(split_login("ftp://h/f", Some(&bad)).is_err());
        assert!(split_login("ftp://u:p%0D%0A@h/f", None).is_err());
    }
}
