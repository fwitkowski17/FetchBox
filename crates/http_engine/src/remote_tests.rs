//! FTP and FTPS through the whole engine, against the test server (`testftp`).

use std::collections::HashMap;
use std::time::Duration;

use fetchbox_core::limiter::RateLimiter;
use fetchbox_core::{Credentials, Downloader, Id, Job};
use tokio::sync::mpsc;

use crate::testftp::{FtpServer, Options, Security};
use crate::{EngineError, Event, HttpDownloader, UrlPolicy};

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

fn engine() -> (
    tempfile::TempDir,
    HttpDownloader,
    mpsc::UnboundedReceiver<Event>,
) {
    let dir = tempfile::tempdir().expect("tmp");
    let (dl, rx) =
        HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
    (dir, dl, rx)
}

fn job(id: Id, url: String, name: &str) -> Job {
    Job {
        id,
        url,
        filename: name.into(),
        subdir: String::new(),
        connections: 4,
        speed_limit: 0,
        etag: None,
        segments: Vec::new(),
        overwrite: false,
        ignore_space: false,
        credentials: None,
    }
}

fn login(user: &str, password: &str) -> Credentials {
    Credentials {
        username: user.into(),
        password: password.into(),
    }
}

fn users(user: &str, password: &str) -> HashMap<String, String> {
    HashMap::from([(user.to_owned(), password.to_owned())])
}

async fn finish(rx: &mut mpsc::UnboundedReceiver<Event>) -> Event {
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .expect("timed out waiting for an event")
            .expect("channel closed");
        if matches!(ev, Event::Completed { .. } | Event::Failed { .. }) {
            return ev;
        }
    }
}

fn completed(ev: &Event) -> bool {
    matches!(ev, Event::Completed { .. })
}

#[tokio::test]
async fn an_anonymous_file_is_downloaded_whole() {
    let body = data(300_000);
    let server = FtpServer::spawn(&[("pub/a.bin", body.clone())], Options::default()).await;
    let (dir, dl, mut rx) = engine();

    let info = dl
        .probe_url(&server.url("ftp", "pub/a.bin"), None)
        .await
        .expect("probe");
    assert_eq!(info.size, Some(300_000));
    assert!(info.resumable);
    assert_eq!(info.filename, "a.bin");
    assert_eq!(info.etag.as_deref(), Some("mtime:20240131120000"));

    dl.start(&job(1, server.url("ftp", "pub/a.bin"), "a.bin"))
        .await
        .expect("start");
    // FTP uses one connection however many are asked for.
    let started = loop {
        match rx.recv().await.expect("event") {
            Event::Started { segments, .. } => break segments,
            Event::Failed { error, .. } => panic!("failed: {error}"),
            _ => {}
        }
    };
    assert_eq!(started.len(), 1);
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
}

#[tokio::test]
async fn a_login_is_used_and_a_wrong_password_is_refused_at_once() {
    let body = data(10_000);
    let options = Options {
        users: users("filip", "p@ss word"),
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", body.clone())], options).await;
    let (dir, dl, mut rx) = engine();

    // From the address, with escapes.
    let url = format!(
        "ftp://filip:p%40ss%20word@127.0.0.1:{}/a.bin",
        server.addr.port()
    );
    assert_eq!(
        dl.probe_url(&url, None).await.expect("probe").size,
        Some(10_000)
    );
    // Given separately.
    let mut j = job(1, server.url("ftp", "a.bin"), "a.bin");
    j.credentials = Some(login("filip", "p@ss word"));
    dl.start(&j).await.expect("start");
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);

    let wrong = login("filip", "nope");
    let err = dl
        .probe_url(&server.url("ftp", "a.bin"), Some(&wrong))
        .await
        .expect_err("refused");
    assert!(matches!(err, EngineError::Auth(_)), "{err:?}");
    // No login at all is an anonymous one, which this server does not allow.
    let err = dl
        .probe_url(&server.url("ftp", "a.bin"), None)
        .await
        .expect_err("refused");
    assert!(matches!(err, EngineError::Auth(_)), "{err:?}");
    // The password is never put in a log line of the server.
    assert!(server.commands().iter().all(|c| !c.contains("p@ss")));
}

#[tokio::test]
async fn a_download_continues_from_the_stored_offset() {
    let body = data(200_000);
    let server = FtpServer::spawn(&[("a.bin", body.clone())], Options::default()).await;
    let (dir, dl, mut rx) = engine();
    let info = dl
        .probe_url(&server.url("ftp", "a.bin"), None)
        .await
        .expect("probe");
    // The first 50 000 bytes are already in the .part file.
    let mut part = body[..50_000].to_vec();
    part.resize(body.len(), 0);
    std::fs::write(dl.part_path(1, "", "a.bin"), part).expect("part");
    let mut j = job(1, server.url("ftp", "a.bin"), "a.bin");
    j.etag = info.etag;
    j.segments = vec![fetchbox_core::SegmentState {
        idx: 0,
        start: 0,
        end: 199_999,
        downloaded: 50_000,
    }];
    dl.start(&j).await.expect("start");
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
    assert!(
        server.commands().iter().any(|c| c == "REST 50000"),
        "{:?}",
        server.commands()
    );
}

#[tokio::test]
async fn a_cut_connection_is_picked_up_where_it_stopped() {
    let body = data(300_000);
    let options = Options {
        cut_after: vec![100_000],
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", body.clone())], options).await;
    let (dir, dl, mut rx) = engine();
    dl.start(&job(1, server.url("ftp", "a.bin"), "a.bin"))
        .await
        .expect("start");
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
    // The retry did not start over: it asked for the rest.
    let rests: Vec<_> = server
        .commands()
        .into_iter()
        .filter(|c| c.starts_with("REST ") && c != "REST 0")
        .collect();
    assert_eq!(rests.len(), 1, "{:?}", server.commands());
    let offset: u64 = rests[0][5..].parse().expect("offset");
    assert!(offset > 0 && offset <= 100_000, "{offset}");
}

#[tokio::test]
async fn a_refused_connection_is_tried_again() {
    let body = data(50_000);
    let options = Options {
        // The probe is the first connection; the transfer's is the second.
        refuse: vec![1],
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", body.clone())], options).await;
    let (dir, dl, mut rx) = engine();
    dl.start(&job(1, server.url("ftp", "a.bin"), "a.bin"))
        .await
        .expect("start");
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
    assert!(server.connections() >= 3);
}

#[tokio::test]
async fn without_rest_a_file_still_downloads_but_cannot_be_continued() {
    let body = data(80_000);
    let options = Options {
        rest: false,
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", body.clone())], options).await;
    let (dir, dl, mut rx) = engine();
    let info = dl
        .probe_url(&server.url("ftp", "a.bin"), None)
        .await
        .expect("probe");
    assert_eq!(info.size, Some(80_000));
    assert!(!info.resumable);
    dl.start(&job(1, server.url("ftp", "a.bin"), "a.bin"))
        .await
        .expect("start");
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
}

#[tokio::test]
async fn without_size_and_feat_the_file_is_a_stream() {
    let body = data(60_000);
    let options = Options {
        size: false,
        feat: false,
        mlsd: false,
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", body.clone())], options).await;
    let (dir, dl, mut rx) = engine();
    let info = dl
        .probe_url(&server.url("ftp", "a.bin"), None)
        .await
        .expect("probe");
    assert_eq!(info.size, None);
    assert!(!info.resumable);
    dl.start(&job(1, server.url("ftp", "a.bin"), "a.bin"))
        .await
        .expect("start");
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
}

#[tokio::test]
async fn the_address_a_server_names_for_the_data_connection_is_ignored() {
    // Without EPSV the server answers PASV with an address that is not its own (an address the
    // program could be aimed at); the data goes to the control connection's address anyway.
    let body = data(40_000);
    let options = Options {
        epsv: false,
        pasv_ip: Some("10.255.255.1".into()),
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", body.clone())], options).await;
    let (dir, dl, mut rx) = engine();
    dl.start(&job(1, server.url("ftp", "a.bin"), "a.bin"))
        .await
        .expect("start");
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
}

#[tokio::test]
async fn folders_are_listed_whatever_style_the_server_uses() {
    let files = [
        ("pub/zeta.txt", data(10)),
        ("pub/Alpha.iso", data(2048)),
        ("pub/sub/deep.txt", data(1)),
        ("pub/other dir/x.bin", data(1)),
        ("pub/my file.mkv", data(500)),
    ];
    for (mlsd, dos) in [(true, false), (false, false), (false, true)] {
        let options = Options {
            mlsd,
            dos_list: dos,
            ..Options::default()
        };
        let server = FtpServer::spawn(&files, options).await;
        let (_dir, dl, _rx) = engine();
        let found = dl
            .inspect_url(&server.url("ftp", "pub/"), None)
            .await
            .expect("inspect");
        let listing = found.listing.expect("a folder");
        let seen: Vec<_> = listing
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.is_dir, e.size))
            .collect();
        assert_eq!(
            seen,
            [
                ("other dir", true, None),
                ("sub", true, None),
                ("Alpha.iso", false, Some(2048)),
                ("my file.mkv", false, Some(500)),
                ("zeta.txt", false, Some(10)),
            ],
            "mlsd={mlsd} dos={dos}"
        );
        let urls: Vec<_> = listing.entries.iter().map(|e| e.url.clone()).collect();
        assert_eq!(urls[0], server.url("ftp", "pub/other%20dir/"));
        assert_eq!(urls[3], server.url("ftp", "pub/my%20file.mkv"));
        // The address of an item is one that can be inspected in turn.
        let again = dl.inspect_url(&urls[1], None).await.expect("sub folder");
        assert_eq!(again.listing.expect("folder").entries.len(), 1);
        let file = dl.probe_url(&urls[3], None).await.expect("file");
        assert_eq!(file.size, Some(500));
        assert_eq!(file.filename, "my file.mkv");
        // The listing of the root.
        let root = dl
            .inspect_url(&server.url("ftp", ""), None)
            .await
            .expect("root");
        assert_eq!(root.listing.expect("folder").entries.len(), 1);
    }
}

#[tokio::test]
async fn a_folder_without_a_trailing_slash_is_found_by_entering_it() {
    let server = FtpServer::spawn(
        &[
            ("pub/sub dir/a.txt", data(3)),
            ("pub/sub dir/b.txt", data(4)),
        ],
        Options {
            mlsd: false,
            ..Options::default()
        },
    )
    .await;
    let (_dir, dl, _rx) = engine();
    let found = dl
        .inspect_url(&server.url("ftp", "pub/sub%20dir"), None)
        .await
        .expect("inspect");
    let names: Vec<_> = found
        .listing
        .expect("a folder")
        .entries
        .into_iter()
        .map(|e| e.name)
        .collect();
    // The folder was entered and listed where it is, not looked for inside itself.
    assert_eq!(names, ["a.txt", "b.txt"]);
    assert!(server.commands().iter().any(|c| c == "CWD pub/sub dir"));
    assert!(server.commands().iter().any(|c| c == "LIST"));
}

#[tokio::test]
async fn a_cut_transfer_of_unknown_length_is_not_taken_for_a_complete_one() {
    // Without a size there is nothing to compare the bytes with: only the server's last reply
    // (426: aborted) says that the transfer was cut, and the download starts over.
    let body = data(60_000);
    let options = Options {
        size: false,
        feat: false,
        mlsd: false,
        cut_after: vec![10_000],
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", body.clone())], options).await;
    let (dir, dl, mut rx) = engine();
    dl.start(&job(1, server.url("ftp", "a.bin"), "a.bin"))
        .await
        .expect("start");
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
    let transfers = server
        .commands()
        .iter()
        .filter(|c| c.starts_with("RETR"))
        .count();
    assert_eq!(transfers, 2, "{:?}", server.commands());
}

#[tokio::test]
async fn a_folder_is_not_a_file_and_a_missing_name_is_said_so() {
    let server = FtpServer::spawn(&[("pub/a.bin", data(5))], Options::default()).await;
    let (_dir, dl, _rx) = engine();
    let err = dl
        .probe_url(&server.url("ftp", "pub"), None)
        .await
        .expect_err("a folder");
    assert!(matches!(err, EngineError::InvalidUrl(_)), "{err:?}");
    let err = dl
        .probe_url(&server.url("ftp", "pub/nope.bin"), None)
        .await
        .expect_err("missing");
    assert!(matches!(err, EngineError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn a_line_break_in_a_name_never_reaches_the_server() {
    let server = FtpServer::spawn(&[("a.bin", data(5))], Options::default()).await;
    let (_dir, dl, _rx) = engine();
    let url = server.url("ftp", "a.bin%0D%0ADELE%20a.bin");
    let err = dl.probe_url(&url, None).await.expect_err("refused");
    assert!(matches!(err, EngineError::InvalidUrl(_)), "{err:?}");
    let bad = login("user\r\nQUIT", "x");
    let err = dl
        .probe_url(&server.url("ftp", "a.bin"), Some(&bad))
        .await
        .expect_err("refused");
    assert!(matches!(err, EngineError::InvalidUrl(_)), "{err:?}");
    assert_eq!(server.connections(), 0, "refused before connecting");
}

#[tokio::test]
async fn private_addresses_are_blocked_when_the_policy_says_so() {
    let server = FtpServer::spawn(&[("a.bin", data(5))], Options::default()).await;
    let dir = tempfile::tempdir().expect("tmp");
    let (dl, _rx) = HttpDownloader::new(
        dir.path(),
        RateLimiter::new(),
        UrlPolicy {
            block_private: true,
        },
    )
    .expect("new");
    // An address written as a number is refused up front ...
    let err = dl
        .probe_url(&server.url("ftp", "a.bin"), None)
        .await
        .expect_err("blocked");
    assert!(matches!(err, EngineError::InvalidUrl(_)), "{err:?}");
    // ... a name that resolves to one when the name is looked up.
    let named = format!("ftp://localhost:{}/a.bin", server.addr.port());
    let err = dl.probe_url(&named, None).await.expect_err("blocked");
    assert!(err.to_string().contains("blocked"), "{err}");
    assert_eq!(server.connections(), 0);
}

#[tokio::test]
async fn ftps_protects_the_control_and_the_data_connection() {
    let body = data(120_000);
    let options = Options {
        security: Security::Explicit,
        users: users("filip", "secret"),
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", body.clone())], options).await;
    let (dir, dl, mut rx) = engine();
    let mut j = job(1, server.url("ftps", "a.bin"), "a.bin");
    j.credentials = Some(login("filip", "secret"));
    dl.start(&j).await.expect("start");
    assert!(completed(&finish(&mut rx).await));
    assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
    let commands = server.commands();
    assert_eq!(commands[0], "AUTH TLS", "{commands:?}");
    assert!(commands.iter().any(|c| c == "PROT P"));
}

#[tokio::test]
async fn ftps_never_falls_back_to_a_plain_connection() {
    let server = FtpServer::spawn(&[("a.bin", data(5))], Options::default()).await;
    let (_dir, dl, _rx) = engine();
    let err = dl
        .probe_url(&server.url("ftps", "a.bin"), None)
        .await
        .expect_err("no TLS");
    assert!(err.to_string().contains("does not offer TLS"), "{err}");
    // Nothing was sent in the clear after the refusal: no login.
    assert!(
        server
            .commands()
            .iter()
            .all(|c| !c.starts_with("USER") && !c.starts_with("PASS")),
        "{:?}",
        server.commands()
    );
}

#[tokio::test]
async fn an_untrusted_certificate_is_refused() {
    // The certificate is one that no client was told to trust.
    let options = Options {
        security: Security::Explicit,
        trusted_cert: false,
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", data(5))], options).await;
    let (_dir, dl, _rx) = engine();
    let err = dl
        .probe_url(&server.url("ftps", "a.bin"), None)
        .await
        .expect_err("untrusted");
    assert!(err.to_string().contains("TLS"), "{err}");
}

#[tokio::test]
async fn implicit_tls_is_spoken_from_the_first_byte() {
    let body = data(30_000);
    let options = Options {
        security: Security::Implicit,
        ..Options::default()
    };
    let server = FtpServer::spawn(&[("a.bin", body.clone())], options).await;
    let address = crate::source::parse(&server.url("ftps", "a.bin")).expect("url");
    let mut target = crate::ftp::Target::new(&address, None);
    // The port is a random one: say that it is the implicit kind of server.
    target.tls = crate::ftp::Tls::Implicit;
    let found = crate::ftp::inspect(UrlPolicy::default(), &target, &address)
        .await
        .expect("inspect");
    match found {
        crate::source::Inspected::File { size, .. } => assert_eq!(size, Some(30_000)),
        crate::source::Inspected::Dir(_) => panic!("a file"),
    }
}

// ---- SFTP ---------------------------------------------------------------------------------

mod sftp {
    use std::sync::Arc;

    use async_trait::async_trait;
    use fetchbox_core::HostKeyProblemKind;

    use super::*;
    use crate::testsftp::{Options, SftpServer};
    use crate::HostKeys;

    /// A list of trusted hosts with one entry.
    struct Trusting(Option<String>);

    #[async_trait]
    impl HostKeys for Trusting {
        async fn trusted(&self, _host: &str, _port: u16) -> Option<String> {
            self.0.clone()
        }
    }

    fn engine_trusting(
        fingerprint: Option<&str>,
        policy: UrlPolicy,
    ) -> (
        tempfile::TempDir,
        HttpDownloader,
        mpsc::UnboundedReceiver<Event>,
    ) {
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, rx) = HttpDownloader::with_host_keys(
            dir.path(),
            RateLimiter::new(),
            policy,
            Arc::new(Trusting(fingerprint.map(str::to_owned))),
        )
        .expect("new");
        (dir, dl, rx)
    }

    fn options() -> Options {
        Options {
            users: users("filip", "p@ss"),
            ..Options::default()
        }
    }

    fn me() -> Credentials {
        login("filip", "p@ss")
    }

    #[tokio::test]
    async fn a_server_that_is_not_trusted_is_refused_before_any_login() {
        let server = SftpServer::spawn(&[("/srv/a.bin", data(100))], options()).await;
        let (_dir, dl, _rx) = engine_trusting(None, UrlPolicy::default());
        let err = dl
            .probe_url(&server.url("srv/a.bin"), Some(&me()))
            .await
            .expect_err("untrusted");
        let EngineError::HostKey(problem) = err else {
            panic!("{err:?}");
        };
        assert_eq!(problem.kind, HostKeyProblemKind::Untrusted);
        assert_eq!(problem.fingerprint, server.fingerprint);
        assert_eq!(problem.key_type, "ssh-ed25519");
        assert_eq!(
            (problem.host.as_str(), problem.port),
            ("127.0.0.1", server.addr.port())
        );
        // The password was not sent to a server nobody had vouched for.
        assert_eq!(server.logins(), 0);
    }

    #[tokio::test]
    async fn a_changed_host_key_is_refused_and_names_both_keys() {
        let server = SftpServer::spawn(&[("/srv/a.bin", data(100))], options()).await;
        let (_dir, dl, _rx) = engine_trusting(Some("SHA256:oldkey"), UrlPolicy::default());
        let err = dl
            .probe_url(&server.url("srv/a.bin"), Some(&me()))
            .await
            .expect_err("changed");
        let EngineError::HostKey(problem) = err else {
            panic!("{err:?}");
        };
        assert_eq!(problem.kind, HostKeyProblemKind::Changed);
        assert_eq!(
            problem.trusted_fingerprint.as_deref(),
            Some("SHA256:oldkey")
        );
        assert_eq!(problem.fingerprint, server.fingerprint);
        assert_eq!(server.logins(), 0);
    }

    #[tokio::test]
    async fn a_trusted_server_is_probed_and_a_file_downloaded() {
        let body = data(300_000);
        let server = SftpServer::spawn(&[("/srv/a.bin", body.clone())], options()).await;
        let (dir, dl, mut rx) = engine_trusting(Some(&server.fingerprint), UrlPolicy::default());
        let info = dl
            .probe_url(&server.url("srv/a.bin"), Some(&me()))
            .await
            .expect("probe");
        assert_eq!(info.size, Some(300_000));
        assert!(info.resumable);
        assert_eq!(info.filename, "a.bin");
        assert_eq!(info.etag.as_deref(), Some("mtime:1700000000"));

        let mut j = job(1, server.url("srv/a.bin"), "a.bin");
        j.credentials = Some(me());
        dl.start(&j).await.expect("start");
        let segments = loop {
            match rx.recv().await.expect("event") {
                Event::Started { segments, .. } => break segments,
                Event::Failed { error, .. } => panic!("failed: {error}"),
                _ => {}
            }
        };
        assert_eq!(segments.len(), 1, "one connection, though 4 were asked for");
        assert!(completed(&finish(&mut rx).await));
        assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
    }

    #[tokio::test]
    async fn the_login_may_be_written_into_the_address() {
        let server = SftpServer::spawn(&[("/srv/a.bin", data(10))], options()).await;
        let (_dir, dl, _rx) = engine_trusting(Some(&server.fingerprint), UrlPolicy::default());
        let url = format!(
            "sftp://filip:p%40ss@127.0.0.1:{}/srv/a.bin",
            server.addr.port()
        );
        assert_eq!(
            dl.probe_url(&url, None).await.expect("probe").size,
            Some(10)
        );
    }

    #[tokio::test]
    async fn a_wrong_password_and_a_missing_login_are_login_errors() {
        let server = SftpServer::spawn(&[("/srv/a.bin", data(10))], options()).await;
        let (_dir, dl, _rx) = engine_trusting(Some(&server.fingerprint), UrlPolicy::default());
        let wrong = login("filip", "nope");
        let err = dl
            .probe_url(&server.url("srv/a.bin"), Some(&wrong))
            .await
            .expect_err("refused");
        assert!(matches!(err, EngineError::Auth(_)), "{err:?}");
        // SFTP has no anonymous login: asked for without one, it does not even connect.
        let err = dl
            .probe_url(&server.url("srv/a.bin"), None)
            .await
            .expect_err("no login");
        assert!(matches!(err, EngineError::Auth(_)), "{err:?}");
        assert_eq!(server.logins(), 0);
    }

    #[tokio::test]
    async fn folders_are_listed_and_names_are_unescaped_and_escaped_again() {
        let server = SftpServer::spawn(
            &[
                ("/srv/zeta.txt", data(10)),
                ("/srv/my file.mkv", data(500)),
                ("/srv/sub/deep.txt", data(1)),
            ],
            options(),
        )
        .await;
        let (_dir, dl, _rx) = engine_trusting(Some(&server.fingerprint), UrlPolicy::default());
        let found = dl
            .inspect_url(&server.url("srv/"), Some(&me()))
            .await
            .expect("inspect");
        let listing = found.listing.expect("a folder");
        let seen: Vec<_> = listing
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.is_dir, e.size))
            .collect();
        assert_eq!(
            seen,
            [
                ("sub", true, None),
                ("my file.mkv", false, Some(500)),
                ("zeta.txt", false, Some(10)),
            ]
        );
        assert_eq!(listing.entries[1].url, server.url("srv/my%20file.mkv"));
        // A folder is not a file.
        let err = dl
            .probe_url(&server.url("srv/sub"), Some(&me()))
            .await
            .expect_err("a folder");
        assert!(matches!(err, EngineError::InvalidUrl(_)), "{err:?}");
        let err = dl
            .probe_url(&server.url("srv/nope"), Some(&me()))
            .await
            .expect_err("missing");
        assert!(matches!(err, EngineError::NotFound(_)), "{err:?}");
    }

    #[tokio::test]
    async fn a_path_after_a_tilde_is_relative_to_the_home_folder() {
        let server = SftpServer::spawn(&[("/home/user/docs/a.txt", data(7))], options()).await;
        let (_dir, dl, _rx) = engine_trusting(Some(&server.fingerprint), UrlPolicy::default());
        let info = dl
            .probe_url(&server.url("~/docs/a.txt"), Some(&me()))
            .await
            .expect("probe");
        assert_eq!(info.size, Some(7));
        let err = dl
            .probe_url(&server.url("docs/a.txt"), Some(&me()))
            .await
            .expect_err("absolute /docs does not exist");
        assert!(matches!(err, EngineError::NotFound(_)), "{err:?}");
    }

    #[tokio::test]
    async fn a_download_continues_from_the_stored_offset() {
        let body = data(200_000);
        let server = SftpServer::spawn(&[("/srv/a.bin", body.clone())], options()).await;
        let (dir, dl, mut rx) = engine_trusting(Some(&server.fingerprint), UrlPolicy::default());
        let info = dl
            .probe_url(&server.url("srv/a.bin"), Some(&me()))
            .await
            .expect("probe");
        let mut part = body[..64_000].to_vec();
        part.resize(body.len(), 0);
        std::fs::write(dl.part_path(1, "", "a.bin"), part).expect("part");
        let mut j = job(1, server.url("srv/a.bin"), "a.bin");
        j.credentials = Some(me());
        j.etag = info.etag;
        j.segments = vec![fetchbox_core::SegmentState {
            idx: 0,
            start: 0,
            end: 199_999,
            downloaded: 64_000,
        }];
        dl.start(&j).await.expect("start");
        assert!(completed(&finish(&mut rx).await));
        assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
    }

    #[tokio::test]
    async fn a_cut_connection_is_picked_up_where_it_stopped() {
        let body = data(300_000);
        let server = SftpServer::spawn(
            &[("/srv/a.bin", body.clone())],
            Options {
                cut_after: vec![100_000],
                ..options()
            },
        )
        .await;
        let (dir, dl, mut rx) = engine_trusting(Some(&server.fingerprint), UrlPolicy::default());
        let mut j = job(1, server.url("srv/a.bin"), "a.bin");
        j.credentials = Some(me());
        dl.start(&j).await.expect("start");
        assert!(completed(&finish(&mut rx).await));
        assert_eq!(std::fs::read(dir.path().join("a.bin")).expect("file"), body);
        // The probe, the cut transfer and its retry: three logins.
        assert!(server.logins() >= 3, "{}", server.logins());
    }

    #[tokio::test]
    async fn private_addresses_are_blocked_when_the_policy_says_so() {
        let server = SftpServer::spawn(&[("/srv/a.bin", data(5))], options()).await;
        let policy = UrlPolicy {
            block_private: true,
        };
        let (_dir, dl, _rx) = engine_trusting(Some(&server.fingerprint), policy);
        let err = dl
            .probe_url(&server.url("srv/a.bin"), Some(&me()))
            .await
            .expect_err("blocked");
        assert!(matches!(err, EngineError::InvalidUrl(_)), "{err:?}");
        let named = format!("sftp://localhost:{}/srv/a.bin", server.addr.port());
        let err = dl
            .probe_url(&named, Some(&me()))
            .await
            .expect_err("blocked");
        assert!(err.to_string().contains("blocked"), "{err}");
        assert_eq!(server.logins(), 0);
    }
}
