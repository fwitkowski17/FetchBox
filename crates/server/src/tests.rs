//! End-to-end tests: real SQLite, real engine, real HTTP in front of the manager.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::Path as UrlPath;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::IntoResponse;
use axum::routing::get;
use fetchbox_api::service::Service;
use fetchbox_core::{
    AddRequest, Category, ConflictKind, Download, Error, Id, OnConflict, Settings, Status,
};
use futures_util::{stream, StreamExt};

use crate::manager::Manager;

const BODY_LEN: usize = 40 * 1024;
/// 4 * 10^18 bytes (4 EB; SQLite stores sizes as signed 64-bit): what `/huge/...` claims to have.
const HUGE_CLAIM: u64 = 4_000_000_000_000_000_000;

fn body() -> Vec<u8> {
    (0..BODY_LEN).map(|i| (i % 251) as u8).collect()
}

/// `/fast/<name>` answers at once, `/slow/<name>` takes about a second, `/huge/<name>` claims
/// to be four exabytes (more than any disk) and only has the usual 40 KiB, `/claim/<bytes>` claims
/// that many bytes, sends 40 KiB and then hangs, so the download stays "running" for as long as
/// a test needs it to hold its reservation.
async fn spawn_files() -> SocketAddr {
    async fn serve(UrlPath((kind, name)): UrlPath<(String, String)>) -> impl IntoResponse {
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
        // `/img/...` claims to be a JPEG, everything else is a generic binary.
        let content_type = if kind == "img" {
            "image/jpeg"
        } else {
            "application/octet-stream"
        };
        let claimed = match (kind.as_str(), name.parse::<u64>()) {
            ("huge", _) => HUGE_CLAIM.to_string(),
            ("claim", Ok(bytes)) => bytes.to_string(),
            _ => BODY_LEN.to_string(),
        };
        let body = if kind == "claim" {
            Body::from_stream(s.chain(stream::pending()))
        } else {
            Body::from_stream(s)
        };
        (
            [
                (CONTENT_LENGTH, claimed),
                (CONTENT_TYPE, content_type.to_string()),
            ],
            body,
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
        Self::with_policy(fetchbox_http_engine::UrlPolicy::default()).await
    }

    async fn with_policy(policy: fetchbox_http_engine::UrlPolicy) -> Self {
        let dir = tempfile::tempdir().expect("tmp");
        let db = fetchbox_storage::open_in_memory().expect("db");
        let mgr = Manager::start(db, dir.path().to_owned(), policy)
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
            ignore_space: false,
            start_at: None,
            start_now: false,
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
        ignore_space: false,
        start_at: None,
        start_now: false,
    };
    assert!(matches!(h.mgr.add(bad).await, Err(Error::InvalidInput(_))));
    let dead = AddRequest {
        url: "http://127.0.0.1:1/x.bin".into(),
        filename: None,
        category: None,
        on_conflict: OnConflict::Reject,
        ignore_space: false,
        start_at: None,
        start_now: false,
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

    h.mgr.remove(d.id, false).await.expect("remove");
    assert!(h.mgr.list().await.expect("l").is_empty());
    assert!(matches!(
        h.mgr.remove(d.id, false).await,
        Err(Error::NotFound(_))
    ));
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
    h.mgr.remove(d.id, false).await.expect("remove");
    assert_eq!(file(&h.path("keep.bin")), body());
}

#[tokio::test]
async fn removing_with_delete_file_deletes_the_finished_file() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("fast", "gone.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Completed).await;
    assert!(h.path("gone.bin").exists());
    h.mgr.remove(d.id, true).await.expect("remove");
    assert!(!h.path("gone.bin").exists(), "the file is deleted");
    assert!(
        h.mgr.list().await.expect("list").is_empty(),
        "and the entry too"
    );

    // A file that is already gone (the user deleted it by hand) is not an error.
    let again = h
        .mgr
        .add(h.req("fast", "vanished.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(again.id, Status::Completed).await;
    std::fs::remove_file(h.path("vanished.bin")).expect("manual delete");
    h.mgr.remove(again.id, true).await.expect("still removable");
}

#[tokio::test]
async fn delete_file_removes_the_renamed_file_in_the_category_directory_only() {
    let h = Harness::new().await;
    let mut s = h.mgr.settings().await.expect("settings");
    s.category_subdirs = true;
    h.mgr.update_settings(s).await.expect("update");
    // A file with the same name that is not ours: it must survive.
    std::fs::create_dir_all(h.path("other")).expect("mkdir");
    std::fs::write(h.path("other/mine.bin"), b"precious").expect("write");
    std::fs::write(h.path("mine.bin"), b"also precious").expect("write");

    let d = h
        .mgr
        .add(h.req("fast", "mine.bin", OnConflict::Rename))
        .await
        .expect("add");
    let done = h.wait_status(d.id, Status::Completed).await;
    assert_eq!(
        done.filename, "mine (1).bin",
        "the engine picked a free name"
    );
    assert!(h.path("other/mine (1).bin").exists());

    h.mgr.remove(d.id, true).await.expect("remove");
    assert!(!h.path("other/mine (1).bin").exists());
    assert_eq!(file(&h.path("other/mine.bin")), b"precious");
    assert_eq!(file(&h.path("mine.bin")), b"also precious");
}

#[tokio::test]
async fn delete_file_is_ignored_for_unfinished_downloads() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("slow", "later.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Downloading).await;
    h.mgr.remove(d.id, true).await.expect("cancel and remove");
    h.mgr.list().await.expect("list");
    assert!(!h.path("later.bin").exists(), "nothing was finished");
    assert!(h.mgr.list().await.expect("list").is_empty());
}

#[tokio::test]
async fn a_download_has_a_speed_limit_of_its_own_that_applies_live() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("slow", "limited.bin", OnConflict::Reject))
        .await
        .expect("add");
    // 2 KiB/s with a one-second burst: the 40 KiB file cannot finish within a couple of seconds
    // (unthrottled it takes about one).
    let limited = h.mgr.set_speed_limit(d.id, 2048).await.expect("limit");
    assert_eq!(limited.speed_limit, 2048);
    tokio::time::sleep(Duration::from_millis(1800)).await;
    let now = h.mgr.list().await.expect("list").remove(0);
    assert_eq!(now.status, Status::Downloading, "throttled, not finished");
    assert!(now.downloaded < BODY_LEN as u64, "{}", now.downloaded);
    assert_eq!(now.speed_limit, 2048, "and the limit is stored");

    // Taking the limit off lets it run at full speed again.
    h.mgr.set_speed_limit(d.id, 0).await.expect("unlimit");
    h.wait_status(d.id, Status::Completed).await;
    assert_eq!(file(&h.path("limited.bin")), body());
}

#[tokio::test]
async fn a_limit_set_while_queued_is_used_when_the_download_starts() {
    let h = Harness::new().await;
    let mut s = h.mgr.settings().await.expect("settings");
    s.max_concurrent = 1;
    h.mgr.update_settings(s).await.expect("update");
    let first = h
        .mgr
        .add(h.req("slow", "first.bin", OnConflict::Reject))
        .await
        .expect("add");
    let second = h
        .mgr
        .add(h.req("slow", "second.bin", OnConflict::Reject))
        .await
        .expect("add");
    assert_eq!(second.status, Status::Queued);
    h.mgr.set_speed_limit(second.id, 2048).await.expect("limit");

    h.wait_status(first.id, Status::Completed).await;
    h.wait_status(second.id, Status::Downloading).await;
    tokio::time::sleep(Duration::from_millis(1800)).await;
    let now = h
        .mgr
        .list()
        .await
        .expect("list")
        .into_iter()
        .find(|d| d.id == second.id)
        .expect("row");
    assert_eq!(
        now.status,
        Status::Downloading,
        "its own limit applies from the start"
    );
    assert!(now.downloaded < BODY_LEN as u64, "{}", now.downloaded);
}

#[tokio::test]
async fn the_default_limit_applies_live_to_downloads_without_a_limit_of_their_own() {
    let h = Harness::new().await;
    let slow = h
        .mgr
        .add(h.req("slow", "default.bin", OnConflict::Reject))
        .await
        .expect("add");
    let mut s = h.mgr.settings().await.expect("settings");
    s.speed_per_download = 2048;
    h.mgr.update_settings(s.clone()).await.expect("default");
    tokio::time::sleep(Duration::from_millis(1800)).await;
    let now = h.mgr.list().await.expect("list").remove(0);
    assert_eq!(now.status, Status::Downloading, "the default throttles it");
    assert_eq!(now.speed_limit, 0, "without storing a limit of its own");

    // A limit of its own replaces the default, here with a higher one: it may be faster.
    h.mgr
        .set_speed_limit(slow.id, 50_000_000)
        .await
        .expect("own limit");
    h.wait_status(slow.id, Status::Completed).await;
    assert_eq!(file(&h.path("default.bin")), body());
}

#[tokio::test]
async fn a_limit_of_its_own_ignores_later_changes_of_the_default() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("slow", "own.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.mgr.set_speed_limit(d.id, 2048).await.expect("own limit");
    let mut s = h.mgr.settings().await.expect("settings");
    s.speed_per_download = 50_000_000; // a faster default must not lift its own limit
    h.mgr.update_settings(s.clone()).await.expect("default");
    tokio::time::sleep(Duration::from_millis(1800)).await;
    let now = h.mgr.list().await.expect("list").remove(0);
    assert_eq!(now.status, Status::Downloading, "still on its own limit");

    // Taking its limit off hands it back to the default (fast here).
    h.mgr.set_speed_limit(d.id, 0).await.expect("unlimit");
    h.wait_status(d.id, Status::Completed).await;
}

#[tokio::test]
async fn the_total_limit_caps_the_sum_of_all_downloads() {
    let h = Harness::new().await;
    let mut s = h.mgr.settings().await.expect("settings");
    s.speed_global = 4096; // far below what two downloads would reach
    h.mgr.update_settings(s).await.expect("total limit");
    let a = h
        .mgr
        .add(h.req("slow", "sum-a.bin", OnConflict::Reject))
        .await
        .expect("add");
    let b = h
        .mgr
        .add(h.req("slow", "sum-b.bin", OnConflict::Reject))
        .await
        .expect("add");
    // A fast limit of its own does not lift the total cap.
    h.mgr.set_speed_limit(a.id, 50_000_000).await.expect("fast");
    tokio::time::sleep(Duration::from_millis(1800)).await;
    let rows = h.mgr.list().await.expect("list");
    let written: u64 = rows.iter().map(|d| d.downloaded).sum();
    assert!(
        rows.iter().all(|d| d.status == Status::Downloading),
        "{rows:?}"
    );
    assert!(
        written < BODY_LEN as u64,
        "{written} bytes for two downloads (a {} byte file each) under a 4 KiB/s total",
        BODY_LEN
    );
    let _ = b;
}

#[tokio::test]
async fn finished_downloads_have_no_speed_to_limit() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("fast", "done.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Completed).await;
    assert!(matches!(
        h.mgr.set_speed_limit(d.id, 1000).await,
        Err(Error::InvalidState(_))
    ));
    assert!(matches!(
        h.mgr.set_speed_limit(9999, 1000).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn patch_sets_the_speed_limit_with_the_other_fields() {
    let h = Harness::new().await;
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();
    let d = h
        .mgr
        .add(h.req("claim", "50000", OnConflict::Reject))
        .await
        .expect("add");
    let patch = |body: &'static str| {
        let (c, url) = (c.clone(), format!("{api}/downloads/{}", d.id));
        async move { raw(&c, reqwest::Method::PATCH, url, Some(body), None).await }
    };
    let r = patch(r#"{"speed_limit":1500000,"category":"video"}"#).await;
    assert_eq!(r.status().as_u16(), 200);
    let text = r.text().await.expect("body");
    assert!(
        text.contains(r#""speed_limit":1500000"#) && text.contains(r#""category":"video""#),
        "{text}"
    );
    let r = patch(r#"{"speed_limit":0}"#).await;
    assert!(r.text().await.expect("body").contains(r#""speed_limit":0"#));
}

#[tokio::test]
async fn finished_downloads_can_be_archived_and_restored() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("fast", "old.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Completed).await;
    assert!(!d.archived);

    let archived = h.mgr.set_archived(d.id, true).await.expect("archive");
    assert!(archived.archived);
    assert_eq!(
        archived.status,
        Status::Completed,
        "the status is untouched"
    );
    assert!(h.path("old.bin").exists(), "and so is the file");
    assert!(h.mgr.list().await.expect("list")[0].archived);

    // Archived entries still count: the same URL is a duplicate, the name is taken.
    assert_eq!(
        conflict_kind(
            h.mgr
                .add(h.req("fast", "old.bin", OnConflict::Reject))
                .await
        ),
        ConflictKind::DuplicateUrl
    );

    let back = h.mgr.set_archived(d.id, false).await.expect("restore");
    assert!(!back.archived);
}

#[tokio::test]
async fn unfinished_downloads_cannot_be_archived() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("slow", "busy.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Downloading).await;
    assert!(matches!(
        h.mgr.set_archived(d.id, true).await,
        Err(Error::InvalidState(_))
    ));
    // Restoring is always harmless, and an unknown id is a 404.
    assert!(h.mgr.set_archived(d.id, false).await.is_ok());
    assert!(matches!(
        h.mgr.set_archived(9999, true).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn patch_changes_category_and_archive_flag_together() {
    let h = Harness::new().await;
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();
    let d = h
        .mgr
        .add(h.req("fast", "both.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Completed).await;

    let patch = |body: &'static str| {
        let (c, url) = (c.clone(), format!("{api}/downloads/{}", d.id));
        async move { raw(&c, reqwest::Method::PATCH, url, Some(body), None).await }
    };
    let both = patch(r#"{"category":"video","archived":true}"#).await;
    assert_eq!(both.status().as_u16(), 200);
    let text = both.text().await.expect("body");
    assert!(
        text.contains(r#""category":"video""#) && text.contains(r#""archived":true"#),
        "{text}"
    );

    assert_eq!(
        patch("{}").await.status().as_u16(),
        400,
        "nothing to update"
    );
    let again = patch(r#"{"archived":false}"#).await;
    assert!(again
        .text()
        .await
        .expect("body")
        .contains(r#""archived":false"#));
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
    spawn_api_with(h, fetchbox_api::Auth::disabled()).await
}

async fn spawn_api_with(h: &Harness, auth: Arc<fetchbox_api::Auth>) -> String {
    let app = fetchbox_api::router(h.mgr.clone(), auth);
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
    assert_eq!(
        stream.headers()["x-accel-buffering"],
        "no",
        "a proxy must not buffer the stream"
    );
    // Something arrives at once: the client counts the stream as open and learns how fast to retry.
    let hello = tokio::time::timeout(Duration::from_secs(2), stream.chunk())
        .await
        .expect("the stream says hello at once")
        .expect("chunk")
        .expect("stream ended");
    let hello = String::from_utf8_lossy(&hello).into_owned();
    assert!(hello.contains("retry: 1000"), "{hello:?}");

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

// ---- disk space -------------------------------------------------------------------

#[tokio::test]
async fn a_file_that_really_does_not_fit_is_added_but_held_back() {
    let h = Harness::new().await;
    // Four exabytes: more than the disk really has, whatever else is queued.
    let d = h
        .mgr
        .add(h.req("huge", "big.bin", OnConflict::Reject))
        .await
        .expect("added, not refused");
    assert_eq!(d.status, Status::Paused, "held back, not started");
    assert!(d.waiting_space);
    assert_eq!(d.size, Some(HUGE_CLAIM));
    assert_eq!(d.downloaded, 0);
    // Nothing was written for it, and it is in the list.
    assert_eq!(std::fs::read_dir(h.dir.path()).expect("ls").count(), 0);
    assert_eq!(h.mgr.list().await.expect("list").len(), 1);

    // Trying again while there is still no room says so and changes nothing.
    let Err(Error::InsufficientSpace {
        needed,
        free,
        reserved,
    }) = h.mgr.resume(d.id).await
    else {
        panic!("resume must be refused while there is no room");
    };
    assert_eq!(needed, HUGE_CLAIM);
    assert!(free < needed);
    assert_eq!(reserved, 0);
    let rows = h.mgr.list().await.expect("list");
    assert!(rows[0].waiting_space && rows[0].status == Status::Paused);

    // It can be cancelled like any other download.
    h.mgr.cancel(d.id).await.expect("cancel");
}

#[tokio::test]
async fn a_held_download_starts_once_there_is_room() {
    let h = Harness::new().await;
    let free = h.mgr.disk_space().await.expect("space").available;
    // Held because it claims more than the disk has free right now...
    let d = h
        .mgr
        .add(h.req("claim", &(free + free / 2).to_string(), OnConflict::Reject))
        .await
        .expect("added");
    assert!(d.waiting_space && d.status == Status::Paused);
    // ...and refused again on resume, while that is still true.
    assert!(matches!(
        h.mgr.resume(d.id).await,
        Err(Error::InsufficientSpace { .. })
    ));
}

/// A fraction of the free space, so these tests work on any disk. The claimed files are sparse
/// and never really written (the server hangs after 40 KiB).
fn share(available: u64, tenths: u64) -> u64 {
    available / 10 * tenths
}

/// Percent of the free space (finer than `share`).
fn pct(available: u64, percent: u64) -> u64 {
    available / 100 * percent
}

#[tokio::test]
async fn running_downloads_reserve_space_for_the_next_ones() {
    let h = Harness::new().await;
    let free = h.mgr.disk_space().await.expect("space").available;
    let claim = |percent: u64| h.req("claim", &pct(free, percent).to_string(), OnConflict::Reject);

    // 60 % of the free space is claimed by the first, running download...
    let first = h.mgr.add(claim(60)).await.expect("first fits");
    let space = h.mgr.disk_space().await.expect("space");
    assert!(
        space.reserved > pct(free, 50) && space.reserved <= pct(free, 60),
        "{space:?}"
    );

    // ...so one of 61 % is refused even though the disk itself is still empty.
    let Err(Error::InsufficientSpace {
        needed,
        free: seen_free,
        reserved,
    }) = h.mgr.add(claim(61)).await
    else {
        panic!("61 % must not fit next to the running 60 %");
    };
    assert_eq!(needed, pct(free, 61));
    // The error lets the UI explain itself: plenty free, but mostly claimed by the first one.
    assert!(seen_free > needed, "the disk itself has room: {seen_free}");
    assert!(reserved > pct(free, 50), "{reserved}");
    assert!(seen_free - reserved < needed);
    assert_eq!(
        h.mgr.list().await.expect("list").len(),
        1,
        "nothing was stored"
    );

    // A smaller one still fits next to it (60 + 30 < 100) and is running too.
    let third = h.mgr.add(claim(30)).await.expect("30 % fits");
    assert!(h.mgr.disk_space().await.expect("space").reserved > pct(free, 80));

    // Pausing gives the reservation back: a paused download is not writing anything.
    h.mgr.pause(third.id).await.expect("pause");
    h.wait_status(third.id, Status::Paused).await;
    let space = h.mgr.disk_space().await.expect("space");
    assert!(space.reserved <= pct(free, 60), "{space:?}");
    h.mgr
        .add(claim(35))
        .await
        .expect("fits now that the 30 % is paused");

    // Cancelling the big one gives its share back as well.
    h.mgr.cancel(first.id).await.expect("cancel");
    assert!(h.mgr.disk_space().await.expect("space").reserved < pct(free, 40));
}

#[tokio::test]
async fn queued_downloads_reserve_nothing() {
    let h = Harness::new().await;
    // One slot: the second download has to wait.
    let mut settings = h.mgr.settings().await.expect("settings");
    settings.max_concurrent = 1;
    h.mgr.update_settings(settings).await.expect("update");

    let free = h.mgr.disk_space().await.expect("space").available;
    let claim = |percent: u64| h.req("claim", &pct(free, percent).to_string(), OnConflict::Reject);
    h.mgr.add(claim(60)).await.expect("running");
    let waiting = h.mgr.add(claim(30)).await.expect("queued");
    assert_eq!(waiting.status, Status::Queued);

    // Only the running one counts, so the queued 30 % does not stand in the way of a 35 %.
    let space = h.mgr.disk_space().await.expect("space");
    assert!(space.reserved <= pct(free, 60), "{space:?}");
    h.mgr
        .add(claim(35))
        .await
        .expect("fits: the queued download holds no space yet");
}

#[tokio::test]
async fn the_user_can_add_a_download_anyway() {
    let h = Harness::new().await;
    let free = h.mgr.disk_space().await.expect("space").available;
    let claim = |tenths: u64, ignore_space: bool| AddRequest {
        ignore_space,
        ..h.req(
            "claim",
            &share(free, tenths).to_string(),
            OnConflict::Reject,
        )
    };

    h.mgr.add(claim(6, false)).await.expect("first");
    assert!(matches!(
        h.mgr.add(claim(7, false)).await,
        Err(Error::InsufficientSpace { .. })
    ));
    // Same request with `ignore_space`: accepted, and the disk is now over-committed, which the
    // ring will show (`reserved` above `available`).
    h.mgr.add(claim(7, true)).await.expect("added anyway");
    let space = h.mgr.disk_space().await.expect("space");
    assert!(space.reserved > space.available, "{space:?}");
    assert_eq!(h.mgr.list().await.expect("list").len(), 2);
}

#[tokio::test]
async fn a_download_added_anyway_is_started_and_not_failed_for_space() {
    let h = Harness::new().await;
    let free = h.mgr.disk_space().await.expect("space").available;
    let claim = |tenths: u64, ignore_space: bool| AddRequest {
        ignore_space,
        ..h.req(
            "claim",
            &share(free, tenths).to_string(),
            OnConflict::Reject,
        )
    };

    // Together they want 130 % of the disk and both start at once (three slots). The engine would
    // refuse the second at its start, since the first is still running, if the user had not said go.
    let first = h.mgr.add(claim(6, false)).await.expect("first");
    let second = h.mgr.add(claim(7, true)).await.expect("anyway");
    h.wait_status(first.id, Status::Downloading).await;
    // `wait_status` panics if the download failed instead.
    h.wait_status(second.id, Status::Downloading).await;

    let flagged = h
        .mgr
        .list()
        .await
        .expect("list")
        .into_iter()
        .filter(|d| d.status == Status::Downloading)
        .count();
    assert_eq!(flagged, 2);
}

#[tokio::test]
async fn disk_endpoint_is_protected_and_refusals_are_507() {
    let h = Harness::new().await;
    let api = spawn_api_with(&h, fetchbox_api::Auth::with_password("correct-horse")).await;
    let c = reqwest::Client::new();
    assert_eq!(aget(&c, &api, "/disk", None).await.status().as_u16(), 401);

    let login = alogin(&c, &api, "correct-horse").await;
    let cookie = session_cookie(&login);
    let ok = aget(&c, &api, "/disk", Some(("cookie", &cookie))).await;
    assert_eq!(ok.status().as_u16(), 200);
    let v: serde_json::Value = serde_json::from_str(&ok.text().await.expect("body")).expect("json");
    assert!(v["total"].as_u64().is_some_and(|n| n > 0), "{v}");
    assert!(v["available"].is_u64() && v["reserved"] == 0, "{v}");

    // 60 % of the free space is reserved by a first download; a second 70 % fits the disk but not
    // next to it: `507` with the figures.
    let free = v["available"].as_u64().expect("available");
    let add = |tenths: u64, extra: &'static str| {
        let url = h.url("claim", &(free / 10 * tenths).to_string());
        format!(r#"{{"url":"{url}"{extra}}}"#)
    };
    let first = apost(
        &c,
        &api,
        "/downloads",
        &add(6, ""),
        Some(("cookie", &cookie)),
    )
    .await;
    assert_eq!(first.status().as_u16(), 201);
    let refused = apost(
        &c,
        &api,
        "/downloads",
        &add(7, ""),
        Some(("cookie", &cookie)),
    )
    .await;
    assert_eq!(refused.status().as_u16(), 507);
    let v: serde_json::Value =
        serde_json::from_str(&refused.text().await.expect("body")).expect("json");
    assert_eq!(v["error"], "insufficient storage");
    assert_eq!(v["needed"].as_u64(), Some(free / 10 * 7));
    assert!(v["free"].as_u64().is_some_and(|n| n > free / 10 * 7), "{v}");
    assert!(v["reserved"].as_u64().is_some_and(|n| n > free / 2), "{v}");

    // The same request with `ignore_space` is accepted over HTTP too.
    let ok = apost(
        &c,
        &api,
        "/downloads",
        &add(7, r#","ignore_space":true"#),
        Some(("cookie", &cookie)),
    )
    .await;
    assert_eq!(ok.status().as_u16(), 201);

    // A file the disk really cannot hold is added, held back, and says so in the list.
    let huge = format!(r#"{{"url":"{}"}}"#, h.url("huge", "big.bin"));
    let held = apost(&c, &api, "/downloads", &huge, Some(("cookie", &cookie))).await;
    assert_eq!(held.status().as_u16(), 201);
    let text = held.text().await.expect("body");
    assert!(text.contains(r#""status":"paused""#), "{text}");
    assert!(text.contains(r#""waiting_space":true"#), "{text}");
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
    let api = spawn_api_with(&h, fetchbox_api::Auth::with_password("correct-horse")).await;
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
    let api = spawn_api_with(&h, fetchbox_api::Auth::with_password("correct-horse")).await;
    let c = reqwest::Client::new();
    for _ in 0..5 {
        assert_eq!(alogin(&c, &api, "wrong").await.status().as_u16(), 401);
    }
    let locked = alogin(&c, &api, "correct-horse").await;
    assert_eq!(locked.status().as_u16(), 429);
    assert!(locked.headers().get("retry-after").is_some());
}

/// POST a JSON body to an auth endpoint.
async fn apost(
    c: &reqwest::Client,
    api: &str,
    path: &str,
    json: &str,
    header: Option<(&str, &str)>,
) -> reqwest::Response {
    raw(
        c,
        reqwest::Method::POST,
        format!("{api}{path}"),
        Some(json),
        header,
    )
    .await
}

#[tokio::test]
async fn delete_over_http_takes_the_file_only_when_asked() {
    let h = Harness::new().await;
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();
    let mut ids = Vec::new();
    for name in ["keep.bin", "drop.bin"] {
        let d = h
            .mgr
            .add(h.req("fast", name, OnConflict::Reject))
            .await
            .expect("add");
        h.wait_status(d.id, Status::Completed).await;
        ids.push(d.id);
    }
    let plain = raw(
        &c,
        reqwest::Method::DELETE,
        format!("{api}/downloads/{}", ids[0]),
        None,
        None,
    )
    .await;
    assert_eq!(plain.status().as_u16(), 204);
    assert!(h.path("keep.bin").exists(), "the file stays by default");

    let with_file = raw(
        &c,
        reqwest::Method::DELETE,
        format!("{api}/downloads/{}?delete_file=true", ids[1]),
        None,
        None,
    )
    .await;
    assert_eq!(with_file.status().as_u16(), 204);
    assert!(!h.path("drop.bin").exists(), "delete_file=true deletes it");
}

#[tokio::test]
async fn first_run_setup_creates_the_account_and_survives_a_restart() {
    let h = Harness::new().await;
    // The manager is the credential store: the hash goes to the real SQLite table.
    let auth = fetchbox_api::Auth::from_store(h.mgr.clone()).expect("auth");
    let code = auth.setup_code().expect("code");
    let api = spawn_api_with(&h, auth).await;
    let c = reqwest::Client::new();

    // Fresh server: locked, and the UI is told to show the setup.
    let st = aget(&c, &api, "/auth/status", None)
        .await
        .text()
        .await
        .expect("body");
    assert!(st.contains(r#""auth_required":true"#), "{st}");
    assert!(st.contains(r#""setup_required":true"#), "{st}");
    assert!(st.contains(r#""can_change_password":false"#), "{st}");
    assert_eq!(
        aget(&c, &api, "/downloads", None).await.status().as_u16(),
        401
    );
    assert_eq!(
        alogin(&c, &api, "anything-goes").await.status().as_u16(),
        401
    );
    assert_eq!(alogin(&c, &api, &code).await.status().as_u16(), 401);

    // Wrong code and weak password create nothing.
    let bad = apost(
        &c,
        &api,
        "/auth/setup",
        r#"{"code":"WRONG123","password":"correct-horse"}"#,
        None,
    )
    .await;
    assert_eq!(bad.status().as_u16(), 403);
    assert!(bad.headers().get("set-cookie").is_none());
    let weak = apost(
        &c,
        &api,
        "/auth/setup",
        &format!(r#"{{"code":"{code}","password":"short"}}"#),
        None,
    )
    .await;
    assert_eq!(weak.status().as_u16(), 400);

    // The right code (typed in lower case) creates the account and logs in.
    let ok = apost(
        &c,
        &api,
        "/auth/setup",
        &format!(
            r#"{{"code":"{}","password":"correct-horse"}}"#,
            code.to_lowercase()
        ),
        None,
    )
    .await;
    assert_eq!(ok.status().as_u16(), 200);
    let cookie = session_cookie(&ok);
    assert!(
        cookie.starts_with("fetchbox_session=") && cookie.len() > 20,
        "{cookie}"
    );
    assert_eq!(
        aget(&c, &api, "/downloads", Some(("cookie", &cookie)))
            .await
            .status()
            .as_u16(),
        200
    );
    let st = aget(&c, &api, "/auth/status", Some(("cookie", &cookie)))
        .await
        .text()
        .await
        .expect("body");
    assert!(st.contains(r#""setup_required":false"#), "{st}");
    assert!(st.contains(r#""can_change_password":true"#), "{st}");
    assert!(st.contains(r#""authenticated":true"#), "{st}");

    // Setup is a one-shot: nobody can take the account over afterwards.
    let again = apost(
        &c,
        &api,
        "/auth/setup",
        &format!(r#"{{"code":"{code}","password":"takeover-pass"}}"#),
        None,
    )
    .await;
    assert_eq!(again.status().as_u16(), 409);
    assert_eq!(
        alogin(&c, &api, "takeover-pass").await.status().as_u16(),
        401
    );

    // Changing the password needs a session, the current password, and ends other sessions.
    let other = alogin(&c, &api, "correct-horse").await;
    assert_eq!(other.status().as_u16(), 200);
    let other_cookie = session_cookie(&other);
    let change = r#"{"current_password":"correct-horse","new_password":"battery-staple"}"#;
    assert_eq!(
        apost(&c, &api, "/auth/password", change, None)
            .await
            .status()
            .as_u16(),
        401
    );
    let wrong = r#"{"current_password":"nope-nope-nope","new_password":"battery-staple"}"#;
    assert_eq!(
        apost(&c, &api, "/auth/password", wrong, Some(("cookie", &cookie)))
            .await
            .status()
            .as_u16(),
        403
    );
    assert_eq!(
        apost(
            &c,
            &api,
            "/auth/password",
            change,
            Some(("cookie", &cookie))
        )
        .await
        .status()
        .as_u16(),
        204
    );
    let status_of = |ck: String| {
        let (c, api) = (c.clone(), api.clone());
        async move {
            aget(&c, &api, "/downloads", Some(("cookie", &ck)))
                .await
                .status()
                .as_u16()
        }
    };
    assert_eq!(status_of(cookie.clone()).await, 200, "the caller stays in");
    assert_eq!(status_of(other_cookie).await, 401, "other sessions end");
    assert_eq!(
        alogin(&c, &api, "correct-horse").await.status().as_u16(),
        401
    );
    assert_eq!(
        alogin(&c, &api, "battery-staple").await.status().as_u16(),
        200
    );

    // "Restart": a new Auth over the same database knows the account and the new password.
    let restarted = fetchbox_api::Auth::from_store(h.mgr.clone()).expect("auth");
    assert!(!restarted.setup_required());
    assert!(restarted.login("battery-staple").is_ok());
    assert!(restarted.login("correct-horse").is_err());
}

#[tokio::test]
async fn externally_configured_password_cannot_be_changed_or_set_up() {
    let h = Harness::new().await;
    let api = spawn_api_with(&h, fetchbox_api::Auth::with_password("correct-horse")).await;
    let c = reqwest::Client::new();
    let ok = alogin(&c, &api, "correct-horse").await;
    let cookie = session_cookie(&ok);

    let st = aget(&c, &api, "/auth/status", Some(("cookie", &cookie)))
        .await
        .text()
        .await
        .expect("body");
    assert!(st.contains(r#""setup_required":false"#), "{st}");
    assert!(st.contains(r#""can_change_password":false"#), "{st}");
    let change = r#"{"current_password":"correct-horse","new_password":"battery-staple"}"#;
    assert_eq!(
        apost(
            &c,
            &api,
            "/auth/password",
            change,
            Some(("cookie", &cookie))
        )
        .await
        .status()
        .as_u16(),
        409
    );
    let setup = r#"{"code":"ABCDEFGH","password":"takeover-pass"}"#;
    assert_eq!(
        apost(&c, &api, "/auth/setup", setup, None)
            .await
            .status()
            .as_u16(),
        409
    );
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

// ---- OpenAPI ------------------------------------------------------------------------

#[test]
fn committed_openapi_matches_the_code() {
    let committed = include_str!("../../../docs/openapi.json");
    let current = fetchbox_api::openapi::spec_json().expect("render");
    assert!(
        committed == current,
        "docs/openapi.json is stale: run ./scripts/gen-api.sh and commit the result"
    );
}

#[tokio::test]
async fn openapi_is_served_publicly_even_with_auth() {
    let h = Harness::new().await;
    let api = spawn_api_with(&h, fetchbox_api::Auth::with_password("correct-horse")).await;
    let c = reqwest::Client::new();
    let resp = aget(&c, &api, "/openapi.json", None).await;
    assert_eq!(resp.status().as_u16(), 200);
    let v: serde_json::Value = serde_json::from_str(&resp.text().await.expect("t")).expect("json");
    assert_eq!(
        v["openapi"].as_str().map(|s| s.starts_with("3.")),
        Some(true)
    );
    assert!(v["paths"]["/api/downloads"].is_object());
}

// ---- category detection, subdirectories, URL policy ---------------------------------

#[tokio::test]
async fn category_falls_back_to_content_type_when_the_name_says_nothing() {
    let h = Harness::new().await;
    let photo = h
        .mgr
        .add(h.req("img", "download", OnConflict::Reject))
        .await
        .expect("add");
    assert_eq!(photo.category, Category::Photo);
    let generic = h
        .mgr
        .add(h.req("bin", "download", OnConflict::Rename))
        .await
        .expect("add");
    assert_eq!(generic.category, Category::Other);
    // The extension is the more reliable signal.
    let by_name = h
        .mgr
        .add(h.req("img", "clip.mkv", OnConflict::Reject))
        .await
        .expect("add");
    assert_eq!(by_name.category, Category::Video);
    // An explicit choice beats both.
    let mut forced = h.req("img", "other.jpg", OnConflict::Reject);
    forced.category = Some(Category::Archive);
    assert_eq!(
        h.mgr.add(forced).await.expect("add").category,
        Category::Archive
    );
}

async fn set_subdirs(h: &Harness, on: bool) {
    let s = Settings {
        category_subdirs: on,
        ..h.mgr.settings().await.expect("s")
    };
    h.mgr.update_settings(s).await.expect("update");
}

#[tokio::test]
async fn category_subdirectories_are_optional_and_decided_at_add_time() {
    let h = Harness::new().await;

    // Off by default: files land in the download directory.
    let flat = h
        .mgr
        .add(h.req("fast", "flat.mkv", OnConflict::Reject))
        .await
        .expect("add");
    assert_eq!(flat.subdir, "");
    h.wait_status(flat.id, Status::Completed).await;
    assert_eq!(file(&h.path("flat.mkv")), body());

    set_subdirs(&h, true).await;
    let video = h
        .mgr
        .add(h.req("fast", "movie.mkv", OnConflict::Reject))
        .await
        .expect("add");
    let music = h
        .mgr
        .add(h.req("fast", "song.mp3", OnConflict::Reject))
        .await
        .expect("add");
    assert_eq!(
        (video.subdir.as_str(), music.subdir.as_str()),
        ("video", "music")
    );
    h.wait_status(video.id, Status::Completed).await;
    h.wait_status(music.id, Status::Completed).await;
    assert_eq!(file(&h.path("video/movie.mkv")), body());
    assert_eq!(file(&h.path("music/song.mp3")), body());
    assert!(!h.path("movie.mkv").exists());

    // Existing downloads stay put when the setting changes or the category is edited.
    set_subdirs(&h, false).await;
    let moved = h
        .mgr
        .set_category(video.id, Category::Other)
        .await
        .expect("cat");
    assert_eq!(moved.subdir, "video");
    assert_eq!(file(&h.path("video/movie.mkv")), body());
}

#[tokio::test]
async fn conflicts_are_checked_inside_the_subdirectory() {
    let h = Harness::new().await;
    set_subdirs(&h, true).await;
    std::fs::create_dir_all(h.path("video")).expect("mkdir");
    std::fs::write(h.path("video/clip.mkv"), b"mine").expect("write");
    // The same name in the base directory is a different file: no conflict with it.
    std::fs::write(h.path("other.mkv"), b"base").expect("write");

    let r = h
        .mgr
        .add(h.req("fast", "clip.mkv", OnConflict::Reject))
        .await;
    let Err(Error::Conflict(c)) = r else {
        panic!("expected conflict")
    };
    assert_eq!(c.kind, ConflictKind::FileExists);
    assert_eq!(c.suggested_name, "clip (1).mkv");

    let d = h
        .mgr
        .add(h.req("fast", "other.mkv", OnConflict::Reject))
        .await
        .expect("no conflict in video/");
    h.wait_status(d.id, Status::Completed).await;
    assert_eq!(file(&h.path("video/other.mkv")), body());
    assert_eq!(file(&h.path("other.mkv")), b"base");
}

#[tokio::test]
async fn private_url_policy_is_applied_by_the_manager() {
    let h = Harness::with_policy(fetchbox_http_engine::UrlPolicy {
        block_private: true,
    })
    .await;
    let r = h.mgr.add(h.req("fast", "a.bin", OnConflict::Reject)).await;
    assert!(matches!(r, Err(Error::InvalidInput(_))), "{r:?}");
    assert!(h.mgr.list().await.expect("l").is_empty());
}

// ---- crash recovery -----------------------------------------------------------------

mod resumable_server {
    use super::*;
    use axum::extract::State;
    use axum::http::header::{ACCEPT_RANGES, CONTENT_RANGE, ETAG, RANGE};
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::Response;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Clone)]
    struct St {
        body: Arc<Vec<u8>>,
        delay: Duration,
        served: Arc<AtomicU64>,
    }

    fn parse_range(v: &str) -> Option<(usize, Option<usize>)> {
        let (a, b) = v.strip_prefix("bytes=")?.split_once('-')?;
        Some((a.parse().ok()?, b.parse().ok()))
    }

    async fn handler(State(st): State<St>, headers: HeaderMap) -> Response {
        let total = st.body.len();
        let range = headers
            .get(RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_range);
        let (from, to, partial) = match range {
            Some((a, b)) => (a, b.unwrap_or(total - 1).min(total - 1), true),
            None => (0, total - 1, false),
        };
        let chunks: Vec<Bytes> = st.body[from..=to]
            .chunks(1024)
            .map(Bytes::copy_from_slice)
            .collect();
        let len = to - from + 1;
        let s = stream::unfold((chunks.into_iter(), st), |(mut it, st)| async move {
            let next = it.next()?;
            tokio::time::sleep(st.delay).await;
            st.served.fetch_add(next.len() as u64, Ordering::Relaxed);
            Some((Ok::<_, std::convert::Infallible>(next), (it, st)))
        });
        let mut b = Response::builder()
            .status(if partial {
                StatusCode::PARTIAL_CONTENT
            } else {
                StatusCode::OK
            })
            .header(CONTENT_LENGTH, len)
            .header(ACCEPT_RANGES, "bytes")
            .header(ETAG, "\"v1\"");
        if partial {
            b = b.header(CONTENT_RANGE, format!("bytes {from}-{to}/{total}"));
        }
        b.body(Body::from_stream(s)).expect("response")
    }

    /// Range-capable file server that counts the body bytes it actually sent.
    pub async fn spawn(len: usize, delay: Duration) -> (SocketAddr, Arc<AtomicU64>, Vec<u8>) {
        let body: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let served = Arc::new(AtomicU64::new(0));
        let st = St {
            body: Arc::new(body.clone()),
            delay,
            served: Arc::clone(&served),
        };
        let app = axum::Router::new()
            .route("/{name}", get(handler))
            .with_state(st);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, served, body)
    }
}

/// A process killed mid-download (no pause, no flush) must pick up from the offsets the
/// last progress tick saved, not start over. Each "process" is its own Tokio runtime that
/// is dropped without waiting, which kills every task it owned.
#[test]
fn killed_process_resumes_from_saved_offsets() {
    use std::sync::atomic::Ordering;

    const LEN: usize = 600 * 1024; // 3 segments of 200 KiB
    let server_rt = tokio::runtime::Runtime::new().expect("rt");
    let (addr, served, body) =
        server_rt.block_on(resumable_server::spawn(LEN, Duration::from_millis(20)));

    let dir = tempfile::tempdir().expect("tmp");
    let db_path = dir.path().join("fetchbox.db");
    let dl_dir = dir.path().join("downloads");
    std::fs::create_dir_all(&dl_dir).expect("mkdir");
    let url = format!("http://{addr}/big.bin");
    let policy = fetchbox_http_engine::UrlPolicy::default();

    // --- process 1: download until progress has been persisted, then "kill -9" it.
    let rt1 = tokio::runtime::Runtime::new().expect("rt");
    let id = rt1.block_on(async {
        let db = fetchbox_storage::open(&db_path).expect("db");
        let mgr = Manager::start(db, dl_dir.clone(), policy)
            .await
            .expect("start");
        let req = AddRequest {
            url: url.clone(),
            filename: None,
            category: None,
            on_conflict: OnConflict::Reject,
            ignore_space: false,
            start_at: None,
            start_now: false,
        };
        let id = mgr.add(req).await.expect("add").id;
        let probe = fetchbox_storage::open(&db_path).expect("second connection");
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let d = fetchbox_storage::downloads::get(&probe, id)
                .expect("get")
                .expect("row");
            if d.status == Status::Downloading && d.downloaded > 0 {
                break;
            }
        }
        // Let the segment offsets of the same tick land too.
        tokio::time::sleep(Duration::from_millis(200)).await;
        id
    });
    rt1.shutdown_background();
    std::thread::sleep(Duration::from_millis(400));

    let probe = fetchbox_storage::open(&db_path).expect("db");
    let crashed = fetchbox_storage::downloads::get(&probe, id)
        .expect("get")
        .expect("row");
    assert_eq!(
        crashed.status,
        Status::Downloading,
        "the crash must look like a crash"
    );
    assert!(
        crashed.downloaded > 0 && (crashed.downloaded as usize) < LEN,
        "{}",
        crashed.downloaded
    );
    let saved: u64 = fetchbox_storage::segments::list(&probe, id)
        .expect("segs")
        .iter()
        .map(|s| s.downloaded)
        .sum();
    assert!(saved > 0, "segment offsets were not persisted");
    drop(probe);
    let served_before = served.load(Ordering::Relaxed);

    // --- process 2: same database, fresh runtime.
    let rt2 = tokio::runtime::Runtime::new().expect("rt");
    rt2.block_on(async {
        let db = fetchbox_storage::open(&db_path).expect("db");
        let mgr = Manager::start(db, dl_dir.clone(), policy)
            .await
            .expect("restart");
        for _ in 0..200 {
            let done = mgr
                .list()
                .await
                .expect("list")
                .into_iter()
                .find(|d| d.id == id);
            if let Some(d) = done {
                assert_ne!(d.status, Status::Failed, "{:?}", d.error);
                if d.status == Status::Completed {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("download did not complete after the restart");
    });
    rt2.shutdown_background();

    assert_eq!(std::fs::read(dl_dir.join("big.bin")).expect("file"), body);
    let sent_after_restart = served.load(Ordering::Relaxed) - served_before;
    assert!(
        (sent_after_restart as usize) < LEN * 85 / 100,
        "restarted from scratch: re-sent {sent_after_restart} of {LEN} bytes"
    );
    server_rt.shutdown_background();
}

// ---- statistics -------------------------------------------------------------------

#[tokio::test]
async fn a_finished_download_is_in_the_statistics_and_stays_there_after_it_is_removed() {
    use fetchbox_core::StatsRange;
    let h = Harness::new().await;
    let before = h.mgr.stats(StatsRange::All, 0).await.expect("stats");
    assert_eq!(
        (before.completed, before.bytes, before.average_speed),
        (0, 0, 0)
    );

    let d = h
        .mgr
        .add(h.req("slow", "stat.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Completed).await;

    let s = h.mgr.stats(StatsRange::All, 0).await.expect("stats");
    assert_eq!(
        (s.completed, s.failed, s.cancelled, s.in_progress),
        (1, 0, 0, 0)
    );
    assert_eq!(
        (s.completed_bytes, s.bytes),
        (BODY_LEN as u64, BODY_LEN as u64)
    );
    assert!(
        s.average_speed > 0,
        "40 KiB over a second or so of transfer"
    );
    assert!(s.average_speed < BODY_LEN as u64 * 4, "{}", s.average_speed);
    assert_eq!(
        s.largest.as_ref().map(|l| l.name.as_str()),
        Some("stat.bin")
    );
    assert_eq!(s.days.len(), 14);
    assert_eq!(s.days.last().map(|d| d.count), Some(1), "it finished today");
    assert_eq!(s.categories.iter().map(|c| c.count).sum::<u64>(), 1);

    // Gone from the list, not from the numbers.
    h.mgr.remove(d.id, false).await.expect("remove");
    assert!(h.mgr.list().await.expect("list").is_empty());
    let after = h.mgr.stats(StatsRange::All, 0).await.expect("stats");
    assert_eq!((after.completed, after.bytes), (1, BODY_LEN as u64));
    // And the periods work: it finished within the last day.
    assert_eq!(
        h.mgr
            .stats(StatsRange::Today, 0)
            .await
            .expect("today")
            .completed
            + h.mgr
                .stats(StatsRange::Week, 14 * 60)
                .await
                .expect("week")
                .completed,
        2
    );
}

#[tokio::test]
async fn time_spent_paused_is_not_time_spent_downloading() {
    use fetchbox_core::StatsRange;
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("slow", "paused.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Downloading).await;
    h.mgr.pause(d.id).await.expect("pause");
    h.wait_status(d.id, Status::Paused).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    h.mgr.resume(d.id).await.expect("resume");
    h.wait_status(d.id, Status::Completed).await;

    let s = h.mgr.stats(StatsRange::All, 0).await.expect("stats");
    // The whole thing took more than three seconds; transferring it took about one.
    assert!(
        s.active_seconds <= 2,
        "{} s counted, three of them were a pause",
        s.active_seconds
    );
    assert!(s.average_speed > 0);
}

#[tokio::test]
async fn cancelling_and_removing_an_unfinished_download_are_recorded_as_cancelled() {
    use fetchbox_core::StatsRange;
    let h = Harness::new().await;
    let cancelled = h
        .mgr
        .add(h.req("claim", "5000000", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(cancelled.id, Status::Downloading).await;
    h.mgr.cancel(cancelled.id).await.expect("cancel");

    let removed = h
        .mgr
        .add(h.req("claim", "6000000", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(removed.id, Status::Downloading).await;
    h.mgr
        .remove(removed.id, false)
        .await
        .expect("remove while running");

    let s = h.mgr.stats(StatsRange::All, 0).await.expect("stats");
    assert_eq!((s.completed, s.cancelled, s.in_progress), (0, 2, 0));
}

#[tokio::test]
async fn the_stats_endpoint_answers_with_the_summary_and_checks_its_query() {
    let h = Harness::new().await;
    let d = h
        .mgr
        .add(h.req("fast", "api-stat.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Completed).await;
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();

    let (status, v) = send(&c, reqwest::Method::GET, format!("{api}/stats"), None).await;
    assert_eq!(status, 200);
    assert_eq!(v["range"], "all");
    assert_eq!(v["completed"], 1);
    assert_eq!(v["days"].as_array().map(Vec::len), Some(14));

    let (status, v) = send(
        &c,
        reqwest::Method::GET,
        format!("{api}/stats?range=7d&offset=120"),
        None,
    )
    .await;
    assert_eq!((status, v["range"].as_str()), (200, Some("7d")));
    let (status, _) = send(
        &c,
        reqwest::Method::GET,
        format!("{api}/stats?range=yesterday"),
        None,
    )
    .await;
    assert_eq!(status, 400);
    // Anything but a signed-in user is turned away like on the other protected routes.
    let locked = spawn_api_with(
        &h,
        fetchbox_api::Auth::with_password("correct-horse".to_owned()),
    )
    .await;
    let (status, _) = send(&c, reqwest::Method::GET, format!("{locked}/stats"), None).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn the_probe_endpoint_describes_a_url_without_adding_it() {
    let h = Harness::new().await;
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();
    let post = |url: String| format!(r#"{{"url":"{url}"}}"#);

    let (status, v) = send(
        &c,
        reqwest::Method::POST,
        format!("{api}/probe"),
        Some(&post(h.url("fast", "holiday.mkv"))),
    )
    .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["filename"], "holiday.mkv");
    assert_eq!(v["size"], BODY_LEN as u64);
    assert_eq!(v["category"], "video");
    assert!(v["resumable"].is_boolean());
    assert!(
        h.mgr.list().await.expect("list").is_empty(),
        "nothing is added"
    );

    let (status, _) = send(
        &c,
        reqwest::Method::POST,
        format!("{api}/probe"),
        Some(r#"{"url":"ftp://example.com/a.bin"}"#),
    )
    .await;
    assert_eq!(status, 400);

    let locked = spawn_api_with(
        &h,
        fetchbox_api::Auth::with_password("correct-horse".to_owned()),
    )
    .await;
    let (status, _) = send(
        &c,
        reqwest::Method::POST,
        format!("{locked}/probe"),
        Some(&post(h.url("fast", "a.bin"))),
    )
    .await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn the_probe_follows_the_private_url_policy() {
    let h = Harness::with_policy(fetchbox_http_engine::UrlPolicy {
        block_private: true,
    })
    .await;
    let r = h.mgr.probe(&h.url("fast", "a.bin")).await;
    assert!(matches!(r, Err(Error::InvalidInput(_))), "{r:?}");
}

/// A server with a directory index at `/pub/` and an ordinary HTML page at `/blog`.
async fn spawn_index_server() -> String {
    use axum::http::header::CONTENT_TYPE;
    use axum::routing::get;
    const INDEX: &str = r#"<html><head><title>Index of /pub</title></head><body><h1>Index of /pub</h1><pre>
<a href="../">../</a>
<a href="sub/">sub/</a>        12-Jan-2024 10:00     -
<a href="song.mp3">song.mp3</a>   12-Jan-2024 10:00  4096
<a href="notes.txt">notes.txt</a>  12-Jan-2024 10:00  12
</pre></body></html>"#;
    let app = axum::Router::new()
        .route(
            "/pub/",
            get(|| async { ([(CONTENT_TYPE, "text/html; charset=utf-8")], INDEX) }),
        )
        .route(
            "/blog",
            get(|| async {
                (
                    [(CONTENT_TYPE, "text/html")],
                    "<title>A blog</title><a href=\"a.zip\">a</a>",
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn probing_a_directory_index_lists_what_is_in_it() {
    let h = Harness::new().await;
    let site = spawn_index_server().await;

    let info = h.mgr.probe(&format!("{site}/pub/")).await.expect("probe");
    assert_eq!(info.kind, fetchbox_core::UrlKind::Listing);
    let names: Vec<_> = info
        .entries
        .iter()
        .map(|e| (e.name.as_str(), e.is_dir, e.size))
        .collect();
    assert_eq!(
        names,
        vec![
            ("sub", true, None),
            ("song.mp3", false, Some(4096)),
            ("notes.txt", false, Some(12)),
        ]
    );
    assert_eq!(info.entries[1].url, format!("{site}/pub/song.mp3"));
    assert!(h.mgr.list().await.expect("list").is_empty());

    // Any other page is just a file.
    let page = h.mgr.probe(&format!("{site}/blog")).await.expect("probe");
    assert_eq!(page.kind, fetchbox_core::UrlKind::File);
    assert!(page.entries.is_empty());
    let file = h.mgr.probe(&h.url("fast", "a.bin")).await.expect("probe");
    assert_eq!(file.kind, fetchbox_core::UrlKind::File);
}

#[tokio::test]
async fn the_private_url_policy_also_covers_reading_a_listing() {
    let h = Harness::with_policy(fetchbox_http_engine::UrlPolicy {
        block_private: true,
    })
    .await;
    let site = spawn_index_server().await;
    let r = h.mgr.probe(&format!("{site}/pub/")).await;
    assert!(matches!(r, Err(Error::InvalidInput(_))), "{r:?}");
}

#[tokio::test]
async fn the_version_endpoint_reports_a_newer_release_unless_checks_are_off() {
    use axum::routing::get;
    let release = axum::Router::new().route(
        "/latest",
        get(|| async {
            r#"[{"tag_name":"v99.0.0","html_url":"https://github.com/o/r/releases/tag/v99.0.0","body":"- Faster"}]"#
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let source = format!("http://{}/latest", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        let _ = axum::serve(listener, release).await;
    });

    let dir = tempfile::tempdir().expect("tmp");
    let db = fetchbox_storage::open_in_memory().expect("db");
    let mgr = Manager::start_with(
        db,
        dir.path().to_owned(),
        fetchbox_http_engine::UrlPolicy::default(),
        Some(source),
    )
    .await
    .expect("start");
    let api = {
        let app = fetchbox_api::router(mgr.clone(), fetchbox_api::Auth::disabled());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = l.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(l, app).await;
        });
        format!("http://{addr}/api")
    };
    let c = reqwest::Client::new();

    let (status, v) = send(&c, reqwest::Method::GET, format!("{api}/version"), None).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["current"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["enabled"], true);
    assert_eq!(v["latest"], "99.0.0");
    assert_eq!(v["update_available"], true);
    assert_eq!(
        v["release_url"],
        "https://github.com/o/r/releases/tag/v99.0.0"
    );
    assert_eq!(v["notes"][0]["version"], "99.0.0");
    assert_eq!(v["notes"][0]["body"], "- Faster");
    let (status, v) = send(
        &c,
        reqwest::Method::POST,
        format!("{api}/version/check"),
        None,
    )
    .await;
    assert_eq!((status, v["update_available"].as_bool()), (200, Some(true)));

    // Switched off in the settings: nothing is asked, nothing is reported.
    let mut settings = mgr.settings().await.expect("settings");
    assert!(settings.check_updates, "on by default");
    settings.check_updates = false;
    mgr.update_settings(settings).await.expect("save");
    let (_, v) = send(&c, reqwest::Method::GET, format!("{api}/version"), None).await;
    assert_eq!(v["enabled"], false);
    assert_eq!(v["latest"], serde_json::Value::Null);
    assert_eq!(v["update_available"], false);

    // Signed-in users only.
    let locked = {
        let app = fetchbox_api::router(
            mgr.clone(),
            fetchbox_api::Auth::with_password("correct-horse".to_owned()),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = l.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(l, app).await;
        });
        format!("http://{addr}/api")
    };
    let (status, _) = send(&c, reqwest::Method::GET, format!("{locked}/version"), None).await;
    assert_eq!(status, 401);
}

#[test]
fn update_checks_are_on_unless_switched_off() {
    // A client that does not know the field (an older UI, a script) must not switch checks off.
    let body = r#"{"connections":4,"max_concurrent":3,"speed_per_download":0,"speed_global":0}"#;
    let s: Settings = serde_json::from_str(body).expect("settings");
    assert!(s.check_updates);
    assert!(Settings::default().check_updates);
    let off = r#"{"connections":4,"max_concurrent":3,"speed_per_download":0,"speed_global":0,"check_updates":false}"#;
    let s: Settings = serde_json::from_str(off).expect("settings");
    assert!(!s.check_updates);
}

// ---- download hours (SPEC 6.3.2) ----------------------------------------------------

/// Settings whose download hours (in UTC) contain the present minute (`open`) or lie 2-3 hours
/// ahead of it, so far from now that a test cannot cross their edges.
fn hours(open: bool) -> Settings {
    let utc = crate::clock::zone("UTC").expect("utc");
    let now = crate::clock::minute_of_day(crate::clock::now(), &utc).expect("minute");
    let day = 24 * 60;
    let (from, to) = if open {
        ((now + day - 60) % day, (now + 60) % day)
    } else {
        ((now + 120) % day, (now + 180) % day)
    };
    Settings {
        schedule_enabled: true,
        schedule_from: from,
        schedule_to: to,
        schedule_timezone: "UTC".into(),
        ..Settings::default()
    }
}

impl Harness {
    async fn wait_hold(&self, id: Id, hold: Option<fetchbox_core::Hold>) -> Download {
        for _ in 0..200 {
            let d = self
                .mgr
                .list()
                .await
                .expect("list")
                .into_iter()
                .find(|d| d.id == id);
            if let Some(d) = d {
                if d.status == Status::Queued && d.hold == hold {
                    return d;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("timed out waiting for hold {hold:?}");
    }
}

#[tokio::test]
async fn downloads_wait_outside_the_download_hours_and_start_when_they_open() {
    let h = Harness::new().await;
    h.mgr.update_settings(hours(false)).await.expect("settings");

    let d = h
        .mgr
        .add(h.req("fast", "night.bin", OnConflict::Reject))
        .await
        .expect("add");
    assert_eq!(d.status, Status::Queued);
    assert_eq!(d.hold, Some(fetchbox_core::Hold::Window));
    let opens = d.starts_at.expect("the opening time is shown");
    assert!(opens > crate::clock::now(), "{opens}");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let still = h.mgr.list().await.expect("list").remove(0);
    assert_eq!(
        still.status,
        Status::Queued,
        "nothing starts outside the hours"
    );
    assert!(!h.path("night.bin").exists());

    // Saving hours that contain the present minute lets it go at once.
    h.mgr.update_settings(hours(true)).await.expect("settings");
    let done = h.wait_status(d.id, Status::Completed).await;
    assert_eq!(done.hold, None);
    assert_eq!(file(&h.path("night.bin")), body());
}

#[tokio::test]
async fn switching_the_hours_off_releases_the_waiting_downloads() {
    let h = Harness::new().await;
    h.mgr.update_settings(hours(false)).await.expect("settings");
    let d = h
        .mgr
        .add(h.req("fast", "a.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_hold(d.id, Some(fetchbox_core::Hold::Window)).await;
    h.mgr
        .update_settings(Settings::default())
        .await
        .expect("settings");
    h.wait_status(d.id, Status::Completed).await;
}

#[tokio::test]
async fn download_now_ignores_the_hours() {
    let h = Harness::new().await;
    h.mgr.update_settings(hours(false)).await.expect("settings");

    // Chosen when adding...
    let mut req = h.req("fast", "now.bin", OnConflict::Reject);
    req.start_now = true;
    let d = h.mgr.add(req).await.expect("add");
    h.wait_status(d.id, Status::Completed).await;

    // ...or afterwards, from the waiting download.
    let waiting = h
        .mgr
        .add(h.req("fast", "later.bin", OnConflict::Reject))
        .await
        .expect("add");
    assert_eq!(waiting.hold, Some(fetchbox_core::Hold::Window));
    h.mgr.start_now(waiting.id).await.expect("start now");
    h.wait_status(waiting.id, Status::Completed).await;

    // Not for one that is over.
    let r = h.mgr.start_now(waiting.id).await;
    assert!(matches!(r, Err(Error::InvalidState(_))), "{r:?}");
}

#[tokio::test]
async fn a_start_time_holds_a_download_until_it_comes() {
    let h = Harness::new().await;
    let now = crate::clock::now();

    let mut far = h.req("fast", "far.bin", OnConflict::Reject);
    far.start_at = Some(now + 3600);
    let far = h.mgr.add(far).await.expect("add");
    assert_eq!(far.hold, Some(fetchbox_core::Hold::Time));
    assert_eq!(far.starts_at, Some(now + 3600));
    assert_eq!(far.start_at, Some(now + 3600));

    let mut soon = h.req("fast", "soon.bin", OnConflict::Reject);
    soon.start_at = Some(now + 2);
    let soon = h.mgr.add(soon).await.expect("add");
    assert_eq!(soon.status, Status::Queued);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    h.mgr.reevaluate().await; // the schedule's own tick would do this
    h.wait_status(soon.id, Status::Completed).await;
    assert_eq!(
        h.mgr
            .list()
            .await
            .expect("list")
            .iter()
            .find(|d| d.id == far.id)
            .map(|d| d.status),
        Some(Status::Queued),
        "the other one is still waiting"
    );

    // "Download now" ends the wait.
    h.mgr.start_now(far.id).await.expect("start now");
    h.wait_status(far.id, Status::Completed).await;
}

#[tokio::test]
async fn a_start_time_must_make_sense() {
    let h = Harness::new().await;
    let now = crate::clock::now();
    for (at, now_flag) in [
        (Some(now + 32 * 24 * 3600), false),
        (Some(now - 3600), false),
        (Some(now + 600), true),
    ] {
        let mut req = h.req("fast", "x.bin", OnConflict::Reject);
        req.start_at = at;
        req.start_now = now_flag;
        let r = h.mgr.add(req).await;
        assert!(
            matches!(r, Err(Error::InvalidInput(_))),
            "{at:?} {now_flag}: {r:?}"
        );
    }
    assert!(h.mgr.list().await.expect("list").is_empty());
}

#[tokio::test]
async fn hours_that_close_put_a_running_download_back_in_the_queue() {
    let h = Harness::new().await;
    h.mgr.update_settings(hours(true)).await.expect("settings");
    // Claims 100 KB, sends 40 KiB and hangs: it stays "running" for as long as the test needs.
    let d = h
        .mgr
        .add(h.req("claim", "100000", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Downloading).await;

    h.mgr.update_settings(hours(false)).await.expect("close");
    let waiting = h.wait_hold(d.id, Some(fetchbox_core::Hold::Window)).await;
    assert_eq!(
        waiting.status,
        Status::Queued,
        "back in the queue, not paused"
    );

    // The slot is free again: another download "now" runs meanwhile.
    let mut other = h.req("fast", "other.bin", OnConflict::Reject);
    other.start_now = true;
    let other = h.mgr.add(other).await.expect("add");
    h.wait_status(other.id, Status::Completed).await;

    // When the hours open it goes on.
    h.mgr.update_settings(hours(true)).await.expect("open");
    h.wait_status(d.id, Status::Downloading).await;
}

#[tokio::test]
async fn a_download_started_now_is_not_stopped_when_the_hours_close() {
    let h = Harness::new().await;
    h.mgr.update_settings(hours(true)).await.expect("settings");
    let d = h
        .mgr
        .add(h.req("claim", "100000", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_status(d.id, Status::Downloading).await;
    h.mgr.start_now(d.id).await.expect("keep it going");
    h.mgr.update_settings(hours(false)).await.expect("close");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let now = h.mgr.list().await.expect("list").remove(0);
    assert_eq!(now.status, Status::Downloading);
}

#[tokio::test]
async fn pausing_a_waiting_download_pauses_it() {
    let h = Harness::new().await;
    h.mgr.update_settings(hours(false)).await.expect("settings");
    let d = h
        .mgr
        .add(h.req("fast", "a.bin", OnConflict::Reject))
        .await
        .expect("add");
    h.wait_hold(d.id, Some(fetchbox_core::Hold::Window)).await;
    let paused = h.mgr.pause(d.id).await.expect("pause");
    assert_eq!(paused.status, Status::Paused);
    assert_eq!(paused.hold, None);
    // Opening the hours does not start a paused download; resuming it inside them does.
    h.mgr.update_settings(hours(true)).await.expect("open");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.mgr.list().await.expect("list")[0].status, Status::Paused);
    h.mgr.resume(d.id).await.expect("resume");
    h.wait_status(d.id, Status::Completed).await;
}

#[tokio::test]
async fn the_download_hours_are_checked_when_saved() {
    let h = Harness::new().await;
    let mut bad_zone = hours(true);
    bad_zone.schedule_timezone = "Mars/Olympus".into();
    assert!(matches!(
        h.mgr.update_settings(bad_zone).await,
        Err(Error::InvalidInput(_))
    ));
    let mut same = hours(true);
    same.schedule_to = same.schedule_from;
    assert!(matches!(
        h.mgr.update_settings(same).await,
        Err(Error::InvalidInput(_))
    ));
    // Equal ends are only a problem while the hours are on.
    let mut off = Settings::default();
    off.schedule_to = off.schedule_from;
    assert!(h.mgr.update_settings(off).await.is_ok());
    // The hours survive a round trip.
    let saved = h.mgr.update_settings(hours(true)).await.expect("save");
    assert_eq!(h.mgr.settings().await.expect("get"), saved);
}

#[tokio::test]
async fn patch_start_now_over_http() {
    let h = Harness::new().await;
    h.mgr.update_settings(hours(false)).await.expect("settings");
    let d = h
        .mgr
        .add(h.req("fast", "http.bin", OnConflict::Reject))
        .await
        .expect("add");
    let api = spawn_api(&h).await;
    let c = reqwest::Client::new();
    let (status, v) = send(
        &c,
        reqwest::Method::PATCH,
        format!("{api}/downloads/{}", d.id),
        Some(r#"{"start_now":true}"#),
    )
    .await;
    assert_eq!(status, 200, "{v}");
    h.wait_status(d.id, Status::Completed).await;
    let (status, _) = send(
        &c,
        reqwest::Method::PATCH,
        format!("{api}/downloads/{}", d.id),
        Some(r#"{"start_now":true}"#),
    )
    .await;
    assert_eq!(status, 409, "a finished download is not waiting");
}
