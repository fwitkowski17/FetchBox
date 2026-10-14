//! End-to-end tests: real SQLite, real engine, real HTTP in front of the manager.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::Path as UrlPath;
use axum::http::header::CONTENT_LENGTH;
use axum::response::IntoResponse;
use axum::routing::get;
use dlm_api::service::Service;
use dlm_core::{
    AddRequest, Category, ConflictKind, Download, Error, Id, OnConflict, Settings, Status,
};
use futures_util::stream;

use crate::manager::Manager;

const BODY_LEN: usize = 40 * 1024;

fn body() -> Vec<u8> {
    (0..BODY_LEN).map(|i| (i % 251) as u8).collect()
}

/// `/fast/<name>` answers at once, `/slow/<name>` takes about a second.
async fn spawn_files() -> SocketAddr {
    async fn serve(UrlPath((kind, _name)): UrlPath<(String, String)>) -> impl IntoResponse {
        let data = body();
        let slow = kind == "slow";
        let chunks: Vec<Bytes> = data.chunks(1024).map(Bytes::copy_from_slice).collect();
        let s = stream::unfold(chunks.into_iter(), move |mut it| async move {
            let next = it.next()?;
            if slow {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Some((Ok::<_, std::convert::Infallible>(next), it))
        });
        (
            [(CONTENT_LENGTH, BODY_LEN.to_string())],
            Body::from_stream(s),
        )
    }
    let app = axum::Router::new().route("/{kind}/{name}", get(serve));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

struct Harness {
    mgr: Arc<Manager>,
    dir: tempfile::TempDir,
    files: SocketAddr,
}

impl Harness {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tmp");
        let db = dlm_storage::open_in_memory().expect("db");
        let mgr = Manager::start(db, dir.path().to_owned())
            .await
            .expect("start");
        Self {
            mgr,
            dir,
            files: spawn_files().await,
        }
    }

    fn url(&self, kind: &str, name: &str) -> String {
        format!("http://{}/{kind}/{name}", self.files)
    }

    fn req(&self, kind: &str, name: &str, on_conflict: OnConflict) -> AddRequest {
        AddRequest {
            url: self.url(kind, name),
            filename: None,
            category: None,
            on_conflict,
        }
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join(name)
    }

    async fn wait_status(&self, id: Id, want: Status) -> Download {
        for _ in 0..200 {
            let d = self
                .mgr
                .list()
                .await
                .expect("list")
                .into_iter()
                .find(|d| d.id == id);
            if let Some(d) = d {
                if d.status == want {
                    return d;
                }
                assert!(
                    !(d.status == Status::Failed && want != Status::Failed),
                    "download failed: {:?}",
                    d.error
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("timed out waiting for {want:?}");
    }
}

fn conflict_kind(r: Result<Download, Error>) -> ConflictKind {
    match r {
        Err(Error::Conflict(c)) => c.kind,
        other => panic!("expected conflict, got {other:?}"),
    }
}

fn file(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("read file")
}

#[tokio::test]
async fn add_downloads_and_detects_category() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("fast", "movie.mkv", OnConflict::Reject))
        .await
        .expect("add");
    assert_eq!(d.category, Category::Video);
    let done = h.wait_status(d.id, Status::Completed).await;
    assert_eq!(done.filename, "movie.mkv");
    assert_eq!(done.size, Some(BODY_LEN as u64));
    assert_eq!(file(&h.path("movie.mkv")), body());
}

#[tokio::test]
async fn invalid_and_unreachable_urls_are_rejected() {
    let h = Harness::new().await;
    let bad = AddRequest {
        url: "file:///etc/passwd".into(),
        filename: None,
        category: None,
        on_conflict: OnConflict::Reject,
    };
    assert!(matches!(h.mgr.add(bad).await, Err(Error::InvalidInput(_))));
    let dead = AddRequest {
        url: "http://127.0.0.1:1/x.bin".into(),
        filename: None,
        category: None,
        on_conflict: OnConflict::Reject,
    };
    assert!(matches!(h.mgr.add(dead).await, Err(Error::Engine(_))));
}

#[tokio::test]
async fn duplicate_url_offers_all_three_choices() {
    let h = Harness::new().await;
    let first = h
        .mgr
        .add(h.req("fast", "a.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(first.id, Status::Completed).await;
    std::fs::write(h.path("a.bin"), b"old").expect("write");

    // Default: nothing happens, the user is told why.
    let k = conflict_kind(h.mgr.add(h.req("fast", "a.bin", OnConflict::Reject)).await);
    assert_eq!(k, ConflictKind::DuplicateUrl);
    assert_eq!(file(&h.path("a.bin")), b"old");

    // Rename: both files exist.
    let renamed = h
        .mgr
        .add(h.req("fast", "a.bin", OnConflict::Rename))
        .await
        .expect("rename");
    assert_eq!(renamed.filename, "a (1).bin");
    h.wait_status(renamed.id, Status::Completed).await;
    assert_eq!(file(&h.path("a (1).bin")), body());
    assert_eq!(file(&h.path("a.bin")), b"old");

    // Overwrite: the old file is replaced.
    let over = h
        .mgr
        .add(h.req("fast", "a.bin", OnConflict::Overwrite))
        .await
        .expect("overwrite");
    h.wait_status(over.id, Status::Completed).await;
    assert_eq!(file(&h.path("a.bin")), body());
}

#[tokio::test]
async fn existing_file_on_disk_is_reported_and_never_clobbered() {
    let h = Harness::new().await;
    std::fs::write(h.path("x.bin"), b"mine").expect("write");

    let r = h.mgr.add(h.req("fast", "x.bin", OnConflict::Reject)).await;
    let Err(Error::Conflict(c)) = r else {
        panic!("expected conflict")
    };
    assert_eq!(c.kind, ConflictKind::FileExists);
    assert_eq!(c.existing_name, "x.bin");
    assert_eq!(c.existing_size, Some(4));
    assert!(c.existing_modified.is_some());
    assert_eq!(c.suggested_name, "x (1).bin");
    assert!(
        h.mgr.list().await.expect("list").is_empty(),
        "rejected add must not create a row"
    );

    let d = h
        .mgr
        .add(h.req("fast", "x.bin", OnConflict::Rename))
        .await
        .expect("rename");
    h.wait_status(d.id, Status::Completed).await;
    assert_eq!(file(&h.path("x.bin")), b"mine");
    assert_eq!(file(&h.path("x (1).bin")), body());
}

#[tokio::test]
async fn name_used_by_unfinished_download_cannot_be_overwritten() {
    let h = Harness::new().await;
    let first = h
        .mgr
        .add(h.req("slow", "s.bin", OnConflict::Reject))
        .await
        .expect("add");

    // Different URL, same file name.
    let k = conflict_kind(h.mgr.add(h.req("fast", "s.bin", OnConflict::Reject)).await);
    assert_eq!(k, ConflictKind::NameReserved);
    assert!(matches!(
        h.mgr
            .add(h.req("fast", "s.bin", OnConflict::Overwrite))
            .await,
        Err(Error::InvalidInput(_))
    ));
    // Same URL while the first is still running: also protected.
    assert!(matches!(
        h.mgr
            .add(h.req("slow", "s.bin", OnConflict::Overwrite))
            .await,
        Err(Error::InvalidInput(_))
    ));

    let second = h
        .mgr
        .add(h.req("fast", "s.bin", OnConflict::Rename))
        .await
        .expect("rename");
    assert_eq!(second.filename, "s (1).bin");
    h.wait_status(first.id, Status::Completed).await;
    h.wait_status(second.id, Status::Completed).await;
    assert_eq!(file(&h.path("s.bin")), body());
    assert_eq!(file(&h.path("s (1).bin")), body());
}

#[tokio::test]
async fn concurrency_limit_queues_and_applies_live() {
    let h = Harness::new().await;
    let s = Settings {
        max_concurrent: 1,
        ..h.mgr.settings().await.expect("s")
    };
    h.mgr.update_settings(s.clone()).await.expect("update");

    let a = h
        .mgr
        .add(h.req("slow", "a.bin", OnConflict::Reject))
        .await
        .expect("a");
    let b = h
        .mgr
        .add(h.req("slow", "b.bin", OnConflict::Reject))
        .await
        .expect("b");
    h.wait_status(a.id, Status::Downloading).await;
    assert_eq!(
        h.mgr
            .list()
            .await
            .expect("l")
            .iter()
            .find(|d| d.id == b.id)
            .map(|d| d.status),
        Some(Status::Queued)
    );

    // Raising the limit starts the queued one without restarting the first.
    h.mgr
        .update_settings(Settings {
            max_concurrent: 2,
            ..s
        })
        .await
        .expect("update");
    h.wait_status(b.id, Status::Downloading).await;
    h.wait_status(a.id, Status::Completed).await;
    h.wait_status(b.id, Status::Completed).await;
}

#[tokio::test]
async fn queued_download_starts_when_a_slot_frees_up() {
    let h = Harness::new().await;
    let s = Settings {
        max_concurrent: 1,
        ..h.mgr.settings().await.expect("s")
    };
    h.mgr.update_settings(s).await.expect("update");
    let a = h
        .mgr
        .add(h.req("slow", "a.bin", OnConflict::Reject))
        .await
        .expect("a");
    let b = h
        .mgr
        .add(h.req("fast", "b.bin", OnConflict::Reject))
        .await
        .expect("b");
    h.wait_status(a.id, Status::Completed).await;
    h.wait_status(b.id, Status::Completed).await;
}

#[tokio::test]
async fn pause_and_resume_finish_with_correct_content() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("slow", "p.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Downloading).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    h.mgr.pause(d.id).await.expect("pause");
    h.wait_status(d.id, Status::Paused).await;
    assert!(matches!(
        h.mgr.pause(d.id).await,
        Err(Error::InvalidState(_))
    ));

    h.mgr.resume(d.id).await.expect("resume");
    h.wait_status(d.id, Status::Completed).await;
    assert_eq!(file(&h.path("p.bin")), body());
}

#[tokio::test]
async fn pausing_a_queued_download_keeps_it_paused_until_resumed() {
    let h = Harness::new().await;
    let s = Settings {
        max_concurrent: 1,
        ..h.mgr.settings().await.expect("s")
    };
    h.mgr.update_settings(s).await.expect("update");
    let a = h
        .mgr
        .add(h.req("slow", "a.bin", OnConflict::Reject))
        .await
        .expect("a");
    let b = h
        .mgr
        .add(h.req("fast", "b.bin", OnConflict::Reject))
        .await
        .expect("b");
    h.mgr.pause(b.id).await.expect("pause queued");
    h.wait_status(b.id, Status::Paused).await;
    h.wait_status(a.id, Status::Completed).await;
    // Still paused after the slot freed up.
    assert_eq!(
        h.mgr
            .list()
            .await
            .expect("l")
            .iter()
            .find(|d| d.id == b.id)
            .map(|d| d.status),
        Some(Status::Paused)
    );
    h.mgr.resume(b.id).await.expect("resume");
    h.wait_status(b.id, Status::Completed).await;
}

#[tokio::test]
async fn cancel_removes_part_file_and_remove_drops_the_row() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("slow", "c.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Downloading).await;
    let cancelled = h.mgr.cancel(d.id).await.expect("cancel");
    assert_eq!(cancelled.status, Status::Cancelled);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let leftovers: Vec<_> = std::fs::read_dir(h.dir.path()).expect("dir").collect();
    assert!(leftovers.is_empty(), "unexpected files: {leftovers:?}");
    assert!(matches!(
        h.mgr.cancel(d.id).await,
        Err(Error::InvalidState(_))
    ));

    h.mgr.remove(d.id).await.expect("remove");
    assert!(h.mgr.list().await.expect("l").is_empty());
    assert!(matches!(h.mgr.remove(d.id).await, Err(Error::NotFound(_))));
}

#[tokio::test]
async fn removing_a_finished_download_keeps_the_file() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("fast", "keep.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Completed).await;
    h.mgr.remove(d.id).await.expect("remove");
    assert_eq!(file(&h.path("keep.bin")), body());
}

#[tokio::test]
async fn settings_are_validated_and_persisted() {
    let h = Harness::new().await;
    let bad = Settings {
        connections: 0,
        ..Settings::default()
    };
    assert!(matches!(
        h.mgr.update_settings(bad).await,
        Err(Error::InvalidInput(_))
    ));
    let good = Settings {
        connections: 6,
        ..Settings::default()
    };
    h.mgr.update_settings(good.clone()).await.expect("update");
    assert_eq!(h.mgr.settings().await.expect("s"), good);
}

#[tokio::test]
async fn category_can_be_changed_manually() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("fast", "thing.xyz", OnConflict::Reject))
        .await
        .expect("add");
    assert_eq!(d.category, Category::Other);
    let d = h
        .mgr
        .set_category(d.id, Category::Archive)
        .await
        .expect("cat");
    assert_eq!(d.category, Category::Archive);
    assert!(matches!(
        h.mgr.set_category(999, Category::Photo).await,
        Err(Error::NotFound(_))
    ));
}

// ---- over real HTTP ---------------------------------------------------------------

async fn spawn_api(h: &Harness) -> String {
    spawn_api_with(h, dlm_api::Auth::disabled()).await
}

async fn spawn_api_with(h: &Harness, auth: Arc<dlm_api::Auth>) -> String {
    let app = dlm_api::router(h.mgr.clone(), auth);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/api")
}

async fn send(
    c: &reqwest::Client,
    method: reqwest::Method,
    url: String,
    json: Option<&str>,
) -> (u16, serde_json::Value) {
    let mut r = c.request(method, url);
    if let Some(j) = json {
        r = r
            .header("content-type", "application/json")
            .body(j.to_owned());
    }
    let resp = r.send().await.expect("send");
    let status = resp.status().as_u16();
    let text = resp.text().await.expect("text");
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn rest_conflict_flow_over_http() {
    let h = Harness::new().await;
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();
    std::fs::write(h.path("doc.pdf"), b"mine").expect("write");

    let add = |extra: &str| format!(r#"{{"url":"{}"{extra}}}"#, h.url("fast", "doc.pdf"));

    let (status, v) = send(
        &c,
        reqwest::Method::POST,
        format!("{api}/downloads"),
        Some(&add("")),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(v["error"], "conflict");
    assert_eq!(v["conflict"]["kind"], "file_exists");
    assert_eq!(v["conflict"]["existing_size"], 4);
    assert_eq!(v["conflict"]["suggested_name"], "doc (1).pdf");

    let (status, v) = send(
        &c,
        reqwest::Method::POST,
        format!("{api}/downloads"),
        Some(&add(r#","on_conflict":"rename""#)),
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(v["filename"], "doc (1).pdf");
    let id = v["id"].as_i64().expect("id");
    h.wait_status(id, Status::Completed).await;

    let (status, v) = send(&c, reqwest::Method::GET, format!("{api}/downloads"), None).await;
    assert_eq!(status, 200);
    assert_eq!(v.as_array().map(Vec::len), Some(1));
    assert_eq!(v[0]["status"], "completed");

    let (status, _) = send(
        &c,
        reqwest::Method::DELETE,
        format!("{api}/downloads/{id}"),
        None,
    )
    .await;
    assert_eq!(status, 204);
}

#[tokio::test]
async fn rest_errors_and_settings() {
    let h = Harness::new().await;
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();

    let (status, _) = send(
        &c,
        reqwest::Method::POST,
        format!("{api}/downloads/42/pause"),
        None,
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = send(
        &c,
        reqwest::Method::POST,
        format!("{api}/downloads"),
        Some(r#"{"url":"ftp://x/y"}"#),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = send(
        &c,
        reqwest::Method::PATCH,
        format!("{api}/downloads/1"),
        Some("{}"),
    )
    .await;
    assert_eq!(status, 400);

    let (status, v) = send(&c, reqwest::Method::GET, format!("{api}/settings"), None).await;
    assert_eq!((status, v["connections"].as_u64()), (200, Some(4)));
    let bad = r#"{"connections":0,"max_concurrent":3,"speed_per_download":0,"speed_global":0}"#;
    let (status, _) = send(
        &c,
        reqwest::Method::PUT,
        format!("{api}/settings"),
        Some(bad),
    )
    .await;
    assert_eq!(status, 400);
    let ok = r#"{"connections":8,"max_concurrent":2,"speed_per_download":0,"speed_global":0}"#;
    let (status, v) = send(
        &c,
        reqwest::Method::PUT,
        format!("{api}/settings"),
        Some(ok),
    )
    .await;
    assert_eq!((status, v["max_concurrent"].as_u64()), (200, Some(2)));
}

#[tokio::test]
async fn sse_streams_download_events() {
    let h = Harness::new().await;
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();

    let mut stream = c.get(format!("{api}/events")).send().await.expect("sse");
    assert_eq!(stream.status().as_u16(), 200);

    let body = format!(r#"{{"url":"{}"}}"#, h.url("slow", "live.bin"));
    let (status, _) = send(
        &c,
        reqwest::Method::POST,
        format!("{api}/downloads"),
        Some(&body),
    )
    .await;
    assert_eq!(status, 201);

    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !(seen.contains("event: download") && seen.contains("\"status\":\"completed\"")) {
        let chunk = tokio::time::timeout_at(deadline, stream.chunk())
            .await
            .expect("timed out waiting for SSE")
            .expect("chunk")
            .expect("stream ended");
        seen.push_str(&String::from_utf8_lossy(&chunk));
    }
    assert!(seen.contains("event: progress") || seen.contains("\"status\":\"downloading\""));
}

// ---- authentication over real HTTP --------------------------------------------------

async fn raw(
    c: &reqwest::Client,
    method: reqwest::Method,
    url: String,
    json: Option<&str>,
    header: Option<(&str, &str)>,
) -> reqwest::Response {
    let mut r = c.request(method, url);
    if let Some((k, v)) = header {
        r = r.header(k, v);
    }
    if let Some(j) = json {
        r = r
            .header("content-type", "application/json")
            .body(j.to_owned());
    }
    r.send().await.expect("send")
}

async fn aget(
    c: &reqwest::Client,
    api: &str,
    path: &str,
    header: Option<(&str, &str)>,
) -> reqwest::Response {
    raw(
        c,
        reqwest::Method::GET,
        format!("{api}{path}"),
        None,
        header,
    )
    .await
}

async fn alogin(c: &reqwest::Client, api: &str, password: &str) -> reqwest::Response {
    let body = format!(r#"{{"password":"{password}"}}"#);
    raw(
        c,
        reqwest::Method::POST,
        format!("{api}/auth/login"),
        Some(&body),
        None,
    )
    .await
}

fn session_cookie(resp: &reqwest::Response) -> String {
    let set = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .expect("set-cookie")
        .to_owned();
    set.split(';').next().expect("pair").to_owned()
}

#[tokio::test]
async fn api_requires_login_when_a_password_is_set() {
    let h = Harness::new().await;
    let api = spawn_api_with(&h, dlm_api::Auth::with_password("correct-horse")).await;
    let c = reqwest::Client::new();

    // Public: health and auth status. Everything else: 401.
    assert_eq!(aget(&c, &api, "/health", None).await.status().as_u16(), 200);
    let st = aget(&c, &api, "/auth/status", None)
        .await
        .text()
        .await
        .expect("t");
    assert!(st.contains(r#""auth_required":true"#) && st.contains(r#""authenticated":false"#));
    for path in ["/downloads", "/settings", "/events"] {
        assert_eq!(
            aget(&c, &api, path, None).await.status().as_u16(),
            401,
            "{path}"
        );
    }
    assert_eq!(
        aget(
            &c,
            &api,
            "/downloads",
            Some(("authorization", "Bearer forged"))
        )
        .await
        .status()
        .as_u16(),
        401
    );
    let add = raw(
        &c,
        reqwest::Method::POST,
        format!("{api}/downloads"),
        Some(&format!(r#"{{"url":"{}"}}"#, h.url("fast", "x.bin"))),
        None,
    )
    .await;
    assert_eq!(add.status().as_u16(), 401);
    assert!(
        h.mgr.list().await.expect("l").is_empty(),
        "unauthenticated add must not run"
    );

    // Wrong password.
    let bad = raw(
        &c,
        reqwest::Method::POST,
        format!("{api}/auth/login"),
        Some(r#"{"password":"nope"}"#),
        None,
    )
    .await;
    assert_eq!(bad.status().as_u16(), 401);
    assert!(bad.headers().get("set-cookie").is_none());

    // Right password: cookie works (also for SSE), and so does the bearer token.
    let ok = raw(
        &c,
        reqwest::Method::POST,
        format!("{api}/auth/login"),
        Some(r#"{"password":"correct-horse"}"#),
        None,
    )
    .await;
    assert_eq!(ok.status().as_u16(), 200);
    let set = ok
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .expect("cookie")
        .to_owned();
    assert!(
        set.contains("HttpOnly") && set.contains("SameSite=Strict"),
        "{set}"
    );
    let cookie = session_cookie(&ok);
    let body: serde_json::Value = serde_json::from_str(&ok.text().await.expect("t")).expect("json");
    let token = body["token"].as_str().expect("token").to_owned();

    assert_eq!(
        aget(&c, &api, "/downloads", Some(("cookie", &cookie)))
            .await
            .status()
            .as_u16(),
        200
    );
    assert_eq!(
        aget(&c, &api, "/events", Some(("cookie", &cookie)))
            .await
            .status()
            .as_u16(),
        200
    );
    let bearer = format!("Bearer {token}");
    assert_eq!(
        aget(&c, &api, "/settings", Some(("authorization", &bearer)))
            .await
            .status()
            .as_u16(),
        200
    );
    let st = aget(&c, &api, "/auth/status", Some(("cookie", &cookie)))
        .await
        .text()
        .await
        .expect("t");
    assert!(st.contains(r#""authenticated":true"#));

    // Logout ends the session for both transports.
    let out = raw(
        &c,
        reqwest::Method::POST,
        format!("{api}/auth/logout"),
        None,
        Some(("cookie", &cookie)),
    )
    .await;
    assert_eq!(out.status().as_u16(), 204);
    assert_eq!(
        aget(&c, &api, "/downloads", Some(("cookie", &cookie)))
            .await
            .status()
            .as_u16(),
        401
    );
    assert_eq!(
        aget(&c, &api, "/downloads", Some(("authorization", &bearer)))
            .await
            .status()
            .as_u16(),
        401
    );
}

#[tokio::test]
async fn repeated_wrong_passwords_are_throttled() {
    let h = Harness::new().await;
    let api = spawn_api_with(&h, dlm_api::Auth::with_password("correct-horse")).await;
    let c = reqwest::Client::new();
    for _ in 0..5 {
        assert_eq!(alogin(&c, &api, "wrong").await.status().as_u16(), 401);
    }
    let locked = alogin(&c, &api, "correct-horse").await;
    assert_eq!(locked.status().as_u16(), 429);
    assert!(locked.headers().get("retry-after").is_some());
}

#[tokio::test]
async fn disabled_auth_keeps_the_api_open() {
    let h = Harness::new().await;
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();
    let st = raw(
        &c,
        reqwest::Method::GET,
        format!("{api}/auth/status"),
        None,
        None,
    )
    .await;
    let text = st.text().await.expect("t");
    assert!(text.contains(r#""auth_required":false"#) && text.contains(r#""authenticated":true"#));
    assert_eq!(
        raw(
            &c,
            reqwest::Method::GET,
            format!("{api}/downloads"),
            None,
            None
        )
        .await
        .status()
        .as_u16(),
        200
    );
}
