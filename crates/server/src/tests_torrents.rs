//! BitTorrent through the manager and the HTTP API (SPEC 6.13): a real second session seeds, the
//! manager's engine downloads.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use fetchbox_api::service::Service;
use fetchbox_core::Event;
use fetchbox_http_engine::UrlPolicy;
use fetchbox_torrent_engine::{Config, TorrentEngine};

use crate::manager::{Manager, TorrentSetup};

const HASH_A: &str = "0123456789abcdef0123456789abcdef01234567";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

async fn read(r: reqwest::Response) -> (u16, serde_json::Value) {
    let status = r.status().as_u16();
    let text = r.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

struct Rig {
    mgr: Arc<Manager>,
    api: String,
    root: tempfile::TempDir,
    port: u16,
}

impl Rig {
    async fn new() -> Self {
        Self::with_policy(UrlPolicy::default()).await
    }

    async fn with_policy(policy: UrlPolicy) -> Self {
        let root = tempfile::tempdir().expect("tmp");
        let db = fetchbox_storage::open_in_memory().expect("db");
        let mgr = Manager::start(db, root.path().join("downloads"), policy)
            .await
            .expect("start");
        let port = free_port();
        mgr.enable_torrents(TorrentSetup {
            state_dir: root.path().join("data"),
            listen_port: port,
            dht: false,
        })
        .await;
        let app = fetchbox_api::router(mgr.clone(), fetchbox_api::Auth::disabled());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            mgr,
            api: format!("http://{addr}/api"),
            root,
            port,
        }
    }

    fn mine(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }

    async fn get(&self, path: &str) -> (u16, serde_json::Value) {
        let r = reqwest::get(format!("{}{path}", self.api))
            .await
            .expect("get");
        read(r).await
    }

    async fn post(&self, path: &str, json: &str) -> (u16, serde_json::Value) {
        let r = reqwest::Client::new()
            .post(format!("{}{path}", self.api))
            .header("content-type", "application/json")
            .body(json.to_owned())
            .send()
            .await
            .expect("post");
        read(r).await
    }

    async fn post_file(&self, bytes: &[u8]) -> (u16, serde_json::Value) {
        let r = reqwest::Client::new()
            .post(format!("{}/torrents/file", self.api))
            .header("content-type", "application/x-bittorrent")
            .body(bytes.to_vec())
            .send()
            .await
            .expect("post");
        read(r).await
    }

    async fn post_file_at(&self, path: &str, bytes: &[u8]) -> (u16, serde_json::Value) {
        let r = reqwest::Client::new()
            .post(format!("{}{path}", self.api))
            .header("content-type", "application/x-bittorrent")
            .body(bytes.to_vec())
            .send()
            .await
            .expect("post");
        read(r).await
    }

    async fn delete(&self, path: &str) -> u16 {
        reqwest::Client::new()
            .delete(format!("{}{path}", self.api))
            .send()
            .await
            .expect("delete")
            .status()
            .as_u16()
    }

    /// The torrent with this id, from the list.
    async fn torrent(&self, id: i64) -> serde_json::Value {
        let (_, list) = self.get("/torrents").await;
        list.as_array()
            .and_then(|l| l.iter().find(|t| t["id"] == id).cloned())
            .unwrap_or(serde_json::Value::Null)
    }

    async fn wait_status(&self, id: i64, status: &str) -> serde_json::Value {
        for _ in 0..300 {
            let t = self.torrent(id).await;
            if t["status"] == status {
                return t;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!(
            "torrent {id} never became {status}: {}",
            self.torrent(id).await
        );
    }
}

/// A seeding session with `data.bin` in it and the `.torrent` file made from it.
async fn seeder(root: &Path, len: usize) -> (Arc<TorrentEngine>, Bytes, Vec<u8>) {
    let engine = TorrentEngine::start(Config {
        output_dir: root.join("seed/files"),
        state_dir: root.join("seed/state"),
        listen_port: 0,
        down_limit: 0,
        up_limit: 0,
        dht: false,
        client: "seeder".into(),
    })
    .await
    .expect("seeder");
    let content: Vec<u8> = (0..len).map(|i| (i * 7 % 253) as u8).collect();
    let path = root.join("seed/files/data.bin");
    std::fs::write(&path, &content).expect("write");
    let made = librqbit::create_torrent(
        &path,
        librqbit::CreateTorrentOptions {
            piece_length: Some(64 * 1024),
            ..Default::default()
        },
        &librqbit::spawn_utils::BlockingSpawner::new(2),
    )
    .await
    .expect("torrent");
    (engine, made.as_bytes().expect("bytes"), content)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_file_is_downloaded_listed_stopped_started_and_removed() {
    let rig = Rig::new().await;
    let (seed, torrent, content) = seeder(rig.root.path(), 900 * 1024).await;
    let mut events = rig.mgr.subscribe();

    let (status, t) = rig.post_file(&torrent).await;
    assert_eq!(status, 201, "{t}");
    let id = t["id"].as_i64().expect("id");
    assert_eq!(t["name"], "data.bin");
    assert_eq!(t["size"], content.len());
    assert_eq!(t["file_count"], 1);
    assert_eq!(t["info_hash"].as_str().map(str::len), Some(40));
    assert!(t["magnet"].is_null());

    // The same torrent twice is refused, and nothing is added.
    let (status, e) = rig.post_file(&torrent).await;
    assert_eq!(status, 409, "{e}");
    assert_eq!(
        rig.get("/torrents").await.1.as_array().map(Vec::len),
        Some(1)
    );

    // The list announced it as an event.
    let first = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(Event::Torrent { torrent }) = events.recv().await {
                return torrent;
            }
        }
    })
    .await
    .expect("a torrent event");
    assert_eq!(first.id, id);

    // The seeder dials in (the torrent has no tracker and there is no DHT here).
    let hash = t["info_hash"].as_str().expect("hash").to_owned();
    seed.add_file(torrent.clone(), false, vec![rig.mine()])
        .await
        .expect("seed");
    let done = rig.wait_status(id, "seeding").await;
    assert_eq!(done["downloaded"], content.len());
    assert_eq!(done["down_speed"], 0, "a complete torrent receives nothing");
    // The moment of finishing is written by the next tick.
    for _ in 0..50 {
        if rig.torrent(id).await["finished_at"].is_i64() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(rig.torrent(id).await["finished_at"].is_i64());
    let file = rig.root.path().join("downloads/torrents/data.bin");
    assert!(std::fs::read(&file).expect("the file") == content);

    let (status, files) = rig.get(&format!("/torrents/{id}/files")).await;
    assert_eq!(status, 200);
    assert_eq!(files[0]["path"], "data.bin");
    assert_eq!(files[0]["downloaded"], files[0]["size"]);
    let (status, peers) = rig.get(&format!("/torrents/{id}/peers")).await;
    assert_eq!(status, 200);
    assert!(
        peers["peers"].is_array() && peers["known"].is_u64(),
        "{peers}"
    );

    // Stopped and complete is "finished"; started again it seeds.
    let (status, stopped) = rig.post(&format!("/torrents/{id}/pause"), "").await;
    assert_eq!(status, 200);
    assert_eq!(stopped["status"], "finished", "{stopped}");
    assert_eq!(
        rig.get(&format!("/torrents/{id}/peers")).await.1["peers"],
        serde_json::json!([])
    );
    let (status, _) = rig.post(&format!("/torrents/{id}/resume"), "").await;
    assert_eq!(status, 200);
    rig.wait_status(id, "seeding").await;

    // Removed with its files.
    assert_eq!(
        rig.delete(&format!("/torrents/{id}?delete_files=true"))
            .await,
        204
    );
    assert!(!file.exists(), "the data went with it");
    assert_eq!(rig.get("/torrents").await.1, serde_json::json!([]));
    assert_eq!(rig.get(&format!("/torrents/{id}/peers")).await.0, 404);
    assert_eq!(rig.delete(&format!("/torrents/{id}")).await, 404);
    let removed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(Event::TorrentRemoved { id }) = events.recv().await {
                return id;
            }
        }
    })
    .await
    .expect("a removed event");
    assert_eq!(removed, id);
    let _ = hash;
    seed.shutdown().await;
    rig.mgr.stop_torrents().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_magnet_link_waits_for_its_description_and_can_be_stopped_and_removed() {
    let rig = Rig::new().await;
    // A tracker, though nobody answers there: without any way to look for peers the engine gives up
    // at once (with the DHT on, as in real use, it would not).
    let uri =
        format!("magnet:?xt=urn:btih:{HASH_A}&dn=Some+Name&tr=udp%3A%2F%2F203.0.113.9%3A6969");
    let (status, t) = rig
        .post("/torrents", &format!(r#"{{"magnet":"{uri}"}}"#))
        .await;
    assert_eq!(status, 201, "{t}");
    let id = t["id"].as_i64().expect("id");
    assert_eq!(
        (t["status"].as_str(), t["name"].as_str()),
        (Some("metadata"), Some("Some Name"))
    );
    assert!(t["size"].is_null());
    assert_eq!(t["magnet"], uri);
    assert_eq!(t["info_hash"], HASH_A);

    // Added twice, and a link in capitals: the same torrent.
    let upper = format!("magnet:?xt=urn:btih:{}", HASH_A.to_uppercase());
    assert_eq!(
        rig.post("/torrents", &format!(r#"{{"magnet":"{upper}"}}"#))
            .await
            .0,
        409
    );

    let (_, files) = rig.get(&format!("/torrents/{id}/files")).await;
    assert_eq!(
        files,
        serde_json::json!([]),
        "no files before the description"
    );

    let (status, p) = rig.post(&format!("/torrents/{id}/pause"), "").await;
    assert_eq!((status, p["status"].as_str()), (200, Some("paused")));
    let (status, r) = rig.post(&format!("/torrents/{id}/resume"), "").await;
    assert_eq!((status, r["status"].as_str()), (200, Some("metadata")));
    assert_eq!(rig.delete(&format!("/torrents/{id}")).await, 204);
    assert_eq!(rig.get("/torrents").await.1, serde_json::json!([]));
    rig.mgr.stop_torrents().await;
}

/// Stand-ins for mounted disks (SPEC 6.14): links to folders on another file system.
#[cfg(unix)]
fn fake_disks(rig: &Rig, names: &[&str]) -> Option<Vec<tempfile::TempDir>> {
    use std::os::unix::fs::MetadataExt;
    let shm = std::path::Path::new("/dev/shm");
    let downloads = rig.root.path().join("downloads");
    std::fs::create_dir_all(&downloads).ok()?;
    if !shm.is_dir()
        || std::fs::metadata(shm).ok()?.dev() == std::fs::metadata(&downloads).ok()?.dev()
    {
        return None;
    }
    names
        .iter()
        .map(|name| {
            let target = tempfile::tempdir_in(shm).ok()?;
            std::os::unix::fs::symlink(target.path(), downloads.join(name)).ok()?;
            Some(target)
        })
        .collect()
}

async fn set_torrent_disks(rig: &Rig, default: &str, torrents: Option<&str>) {
    let mut s = fetchbox_api::service::Service::settings(&*rig.mgr)
        .await
        .expect("settings");
    s.default_disk = default.to_owned();
    s.torrent_default_disk = torrents.map(str::to_owned);
    fetchbox_api::service::Service::update_settings(&*rig.mgr, s)
        .await
        .expect("save");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_is_saved_on_the_chosen_disk() {
    let rig = Rig::new().await;
    let Some(targets) = fake_disks(&rig, &["ssd1", "hdd10tb"]) else {
        return;
    };
    let (ssd, hdd) = (targets[0].path(), targets[1].path());
    let (seed, torrent, content) = seeder(rig.root.path(), 300 * 1024).await;

    // Torrents follow the downloads' default disk until they have one of their own.
    set_torrent_disks(&rig, "ssd1", None).await;
    let r = reqwest::Client::new()
        .post(format!("{}/torrents/file?disk=hdd10tb", rig.api))
        .header("content-type", "application/x-bittorrent")
        .body(torrent.to_vec())
        .send()
        .await
        .expect("post");
    let (status, t) = read(r).await;
    assert_eq!(status, 201, "{t}");
    assert_eq!(t["disk"], "hdd10tb", "the request names a disk");
    let id = t["id"].as_i64().expect("id");
    seed.add_file(torrent.clone(), false, vec![rig.mine()])
        .await
        .expect("seed");
    rig.wait_status(id, "seeding").await;
    // One file goes straight into <disk>/torrents.
    assert!(std::fs::read(hdd.join("torrents/data.bin")).expect("on the disk") == content);
    assert!(!ssd.join("torrents/data.bin").exists());
    assert!(!rig.root.path().join("downloads/torrents/data.bin").exists());

    // Removed with its files from the disk it is on, whatever the default is now.
    set_torrent_disks(&rig, "ssd1", Some("")).await;
    assert_eq!(
        rig.delete(&format!("/torrents/{id}?delete_files=true"))
            .await,
        204
    );
    assert!(!hdd.join("torrents/data.bin").exists());
    seed.shutdown().await;
    rig.mgr.stop_torrents().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_magnet_link_gets_a_folder_on_its_disk_and_the_defaults_are_told_apart() {
    let rig = Rig::new().await;
    let Some(_targets) = fake_disks(&rig, &["ssd1", "hdd10tb"]) else {
        return;
    };
    set_torrent_disks(&rig, "ssd1", None).await;
    let magnet =
        |hash: &str, name: &str| format!(r#"{{"magnet":"magnet:?xt=urn:btih:{hash}&dn={name}"}}"#);
    // No disk named, no disk of their own: the downloads' default.
    let (status, a) = rig.post("/torrents", &magnet(HASH_A, "One")).await;
    assert_eq!(status, 201, "{a}");
    assert_eq!(a["disk"], "ssd1");
    // Torrents with a default of their own.
    set_torrent_disks(&rig, "ssd1", Some("hdd10tb")).await;
    let hash_b = "1123456789abcdef0123456789abcdef01234567";
    let (_, b) = rig.post("/torrents", &magnet(hash_b, "Two")).await;
    assert_eq!(b["disk"], "hdd10tb");
    // The download directory itself, asked for by an empty name.
    let hash_c = "2123456789abcdef0123456789abcdef01234567";
    let (_, c) = rig
        .post(
            "/torrents",
            &format!(r#"{{"magnet":"magnet:?xt=urn:btih:{hash_c}","disk":""}}"#),
        )
        .await;
    assert_eq!(c["disk"], "");
    // A disk that is not there is refused, and nothing is added.
    let hash_d = "3123456789abcdef0123456789abcdef01234567";
    let (status, e) = rig
        .post(
            "/torrents",
            &format!(r#"{{"magnet":"magnet:?xt=urn:btih:{hash_d}","disk":"nope"}}"#),
        )
        .await;
    assert_eq!(status, 400, "{e}");
    assert_eq!(
        rig.get("/torrents").await.1.as_array().map(Vec::len),
        Some(3)
    );
    let (status, _) = rig
        .post_file_at("/torrents/file?disk=..", b"not a torrent")
        .await;
    assert_eq!(status, 400);
    rig.mgr.stop_torrents().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_torrent_whose_disk_is_gone_is_stopped_with_a_note_and_not_started() {
    let rig = Rig::new().await;
    let Some(targets) = fake_disks(&rig, &["hdd10tb"]) else {
        return;
    };
    set_torrent_disks(&rig, "hdd10tb", None).await;
    let (_, t) = rig
        .post(
            "/torrents",
            &format!(r#"{{"magnet":"magnet:?xt=urn:btih:{HASH_A}"}}"#),
        )
        .await;
    let id = t["id"].as_i64().expect("id");
    let (status, _) = rig.post(&format!("/torrents/{id}/pause"), "").await;
    assert_eq!(status, 200);
    // The disk goes away (an empty folder stands in its place).
    std::fs::remove_file(rig.root.path().join("downloads/hdd10tb")).expect("unmount");
    std::fs::create_dir(rig.root.path().join("downloads/hdd10tb")).expect("empty folder");
    let (status, e) = rig.post(&format!("/torrents/{id}/resume"), "").await;
    assert_eq!(status, 400, "{e}");
    assert!(e.to_string().contains("not available"), "{e}");
    assert_eq!(
        std::fs::read_dir(rig.root.path().join("downloads/hdd10tb"))
            .expect("dir")
            .count(),
        0,
        "nothing was written in its place"
    );
    drop(targets);
    rig.mgr.stop_torrents().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_is_not_a_torrent_is_refused() {
    let rig = Rig::new().await;
    for bad in [
        r#"{"magnet":""}"#,
        r#"{"magnet":"https://example.com/a.torrent"}"#,
        r#"{"magnet":"magnet:?dn=no-hash"}"#,
        r#"{"magnet":"magnet:?xt=urn:btih:nothex"}"#,
    ] {
        let (status, e) = rig.post("/torrents", bad).await;
        assert_eq!(status, 400, "{bad}: {e}");
    }
    for bad in [&b""[..], b"not a torrent", b"d4:spam4:eggse"] {
        let (status, e) = rig.post_file(bad).await;
        assert_eq!(status, 400, "{e}");
    }
    let too_big = vec![b'x'; fetchbox_core::MAX_TORRENT_FILE + 4096];
    assert_eq!(rig.post_file(&too_big).await.0, 413);
    assert_eq!(
        rig.get("/torrents").await.1,
        serde_json::json!([]),
        "nothing was added"
    );
    assert_eq!(rig.post("/torrents/99/pause", "").await.0, 404);
    assert_eq!(rig.post("/torrents/99/resume", "").await.0, 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn torrents_are_off_until_the_manager_is_told() {
    let dir = tempfile::tempdir().expect("tmp");
    let db = fetchbox_storage::open_in_memory().expect("db");
    let mgr = Manager::start(db, dir.path().to_owned(), UrlPolicy::default())
        .await
        .expect("start");
    let err = fetchbox_api::service::Service::add_torrent_magnet(
        &*mgr,
        fetchbox_core::AddTorrentRequest {
            magnet: format!("magnet:?xt=urn:btih:{HASH_A}"),
            disk: None,
        },
    )
    .await
    .expect_err("not switched on");
    assert!(err.to_string().contains("not switched on"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trackers_at_private_addresses_are_refused_when_those_are_blocked() {
    let rig = Rig::with_policy(UrlPolicy {
        block_private: true,
    })
    .await;
    for tracker in [
        "http%3A%2F%2F127.0.0.1%3A8080%2Fapi%2Fx",
        "udp%3A%2F%2F10.0.0.5%3A80",
        "http%3A%2F%2F%5B%3A%3A1%5D%3A80%2Fannounce",
        "http%3A%2F%2Flocalhost%3A80%2Fannounce",
    ] {
        let body = format!(r#"{{"magnet":"magnet:?xt=urn:btih:{HASH_A}&tr={tracker}"}}"#);
        let (status, e) = rig.post("/torrents", &body).await;
        assert_eq!(status, 400, "{tracker}: {e}");
        assert!(e["error"].as_str().unwrap_or("").contains("private"), "{e}");
    }
    assert_eq!(rig.get("/torrents").await.1, serde_json::json!([]));
    // A public tracker is fine.
    let ok = format!(r#"{{"magnet":"magnet:?xt=urn:btih:{HASH_A}&tr=udp%3A%2F%2F8.8.8.8%3A80"}}"#);
    assert_eq!(rig.post("/torrents", &ok).await.0, 201);
    rig.mgr.stop_torrents().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_list_and_the_downloads_come_back_after_a_restart() {
    let root = tempfile::tempdir().expect("tmp");
    let (seed, torrent, content) = seeder(root.path(), 400 * 1024).await;
    let db_path = root.path().join("fetchbox.db");
    let downloads = root.path().join("downloads");
    let setup = |port| TorrentSetup {
        state_dir: root.path().join("data"),
        listen_port: port,
        dht: false,
    };

    let first_port = free_port();
    let first = Manager::start(
        fetchbox_storage::open(&db_path).expect("db"),
        downloads.clone(),
        UrlPolicy::default(),
    )
    .await
    .expect("start");
    first.enable_torrents(setup(first_port)).await;
    let added = fetchbox_api::service::Service::add_torrent_file(&*first, torrent.to_vec(), None)
        .await
        .expect("add");
    seed.add_file(
        torrent.clone(),
        false,
        vec![SocketAddr::from(([127, 0, 0, 1], first_port))],
    )
    .await
    .expect("seed");
    wait_until(&first, added.id, |t| {
        t.status == fetchbox_core::TorrentStatus::Seeding
    })
    .await;
    let stopped = fetchbox_api::service::Service::pause_torrent(&*first, added.id)
        .await
        .expect("pause");
    assert_eq!(stopped.status, fetchbox_core::TorrentStatus::Finished);
    first.stop_torrents().await;
    drop(first);

    // A new program on the same database and folders.
    let second = Manager::start(
        fetchbox_storage::open(&db_path).expect("db"),
        downloads.clone(),
        UrlPolicy::default(),
    )
    .await
    .expect("restart");
    second.enable_torrents(setup(free_port())).await;
    let list = fetchbox_api::service::Service::torrents(&*second)
        .await
        .expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].id, list[0].name.as_str()), (added.id, "data.bin"));
    // Stopped when it was left, and complete: finished, with the bytes it sent kept.
    let t = wait_until(&second, added.id, |t| {
        t.status == fetchbox_core::TorrentStatus::Finished
    })
    .await;
    assert_eq!(t.size, Some(content.len() as u64));
    assert!(t.finished_at.is_some());
    // Started again, it checks what is on disk and seeds without downloading anything.
    fetchbox_api::service::Service::resume_torrent(&*second, added.id)
        .await
        .expect("resume");
    wait_until(&second, added.id, |t| {
        t.status == fetchbox_core::TorrentStatus::Seeding && t.downloaded == content.len() as u64
    })
    .await;
    second.stop_torrents().await;
    seed.shutdown().await;
}

/// Torrent hours that surely exclude (or include) the minute it is now, in UTC.
fn hours_around_now(open: bool) -> (u16, u16) {
    let minute = ((crate::clock::now() / 60) % 1440) as u16;
    if open {
        ((minute + 1440 - 60) % 1440, (minute + 60) % 1440)
    } else {
        ((minute + 120) % 1440, (minute + 180) % 1440)
    }
}

async fn set_torrent_hours(mgr: &Arc<Manager>, enabled: bool, open: bool) {
    let (from, to) = hours_around_now(open);
    let mut s = Service::settings(&**mgr).await.expect("settings");
    s.torrent_schedule_enabled = enabled;
    s.torrent_schedule_from = from;
    s.torrent_schedule_to = to;
    s.torrent_schedule_timezone = "UTC".into();
    Service::update_settings(&**mgr, s).await.expect("save");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn torrents_wait_for_their_own_hours_and_can_start_now() {
    let rig = Rig::new().await;
    let (seed, torrent, content) = seeder(rig.root.path(), 300 * 1024).await;
    set_torrent_hours(&rig.mgr, true, false).await;

    // Added outside the hours: it waits, shown as paused with the time the hours open.
    let (status, t) = rig.post_file(&torrent).await;
    assert_eq!(status, 201, "{t}");
    let id = t["id"].as_i64().expect("id");
    assert_eq!(t["status"], "paused", "{t}");
    assert_eq!(t["hold"], "window");
    assert!(t["starts_at"].is_i64(), "{t}");
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let waiting = rig.torrent(id).await;
    assert_eq!(waiting["status"], "paused", "{waiting}");
    assert_eq!(waiting["downloaded"], 0, "nothing moves outside the hours");

    // "Start now" lets this one run although the hours are closed.
    let (status, started) = rig.post(&format!("/torrents/{id}/start-now"), "").await;
    assert_eq!(status, 200, "{started}");
    assert_eq!(started["start_now"], true);
    // The seeder dials in only now: a stopped torrent does not take connections.
    seed.add_file(torrent.clone(), false, vec![rig.mine()])
        .await
        .expect("seed");
    let done = rig.wait_status(id, "seeding").await;
    assert!(done["hold"].is_null() && done["downloaded"] == content.len());

    // Stopping it by hand ends the exemption: started again outside the hours it waits.
    let (_, stopped) = rig.post(&format!("/torrents/{id}/pause"), "").await;
    assert_eq!(stopped["status"], "finished", "{stopped}");
    assert_eq!(stopped["start_now"], false);
    assert_eq!(
        rig.post(&format!("/torrents/{id}/start-now"), "").await.0,
        409,
        "a stopped torrent has to be started first"
    );
    let (_, again) = rig.post(&format!("/torrents/{id}/resume"), "").await;
    assert_eq!(again["status"], "paused", "{again}");
    assert_eq!(again["hold"], "window");

    // The hours open: it goes on by itself; they close: it is stopped again.
    set_torrent_hours(&rig.mgr, true, true).await;
    rig.wait_status(id, "seeding").await;
    set_torrent_hours(&rig.mgr, true, false).await;
    let closed = rig.wait_status(id, "paused").await;
    assert_eq!(closed["hold"], "window");
    // Switching the hours off releases it.
    set_torrent_hours(&rig.mgr, false, false).await;
    let free = rig.wait_status(id, "seeding").await;
    assert!(free["hold"].is_null());

    // A torrent the user stopped is not started by the hours opening.
    rig.post(&format!("/torrents/{id}/pause"), "").await;
    set_torrent_hours(&rig.mgr, true, false).await;
    set_torrent_hours(&rig.mgr, true, true).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(rig.torrent(id).await["status"], "finished");

    seed.shutdown().await;
    rig.mgr.stop_torrents().await;
}

#[test]
fn the_torrent_hours_need_a_start_and_an_end_that_differ() {
    let bad = fetchbox_core::Settings {
        torrent_schedule_enabled: true,
        torrent_schedule_from: 600,
        torrent_schedule_to: 600,
        ..fetchbox_core::Settings::default()
    };
    assert!(bad.validate().is_err());
    let off = fetchbox_core::Settings {
        torrent_schedule_enabled: false,
        ..bad
    };
    assert!(
        off.validate().is_ok(),
        "switched off, the values do not matter"
    );
}

async fn wait_until(
    mgr: &Arc<Manager>,
    id: i64,
    ok: impl Fn(&fetchbox_core::Torrent) -> bool,
) -> fetchbox_core::Torrent {
    for _ in 0..300 {
        let list = fetchbox_api::service::Service::torrents(&**mgr)
            .await
            .expect("list");
        if let Some(t) = list.into_iter().find(|t| t.id == id && ok(t)) {
            return t;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let last = fetchbox_api::service::Service::torrents(&**mgr)
        .await
        .expect("list");
    panic!("the torrent never reached the expected state; the list is: {last:#?}");
}
