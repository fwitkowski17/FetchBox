//! Plugins through the manager (SPEC 6.15): installing, the lifecycle, settings, review of changed
//! files, and actions, against a stand-in plugin program.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use fetchbox_api::service::Service;
use fetchbox_core::plugin::{
    platform_key, PluginList, PluginSource, PluginState, PluginView, SettingValue, PLATFORMS,
};
use fetchbox_core::{Error, Event};
use fetchbox_http_engine::UrlPolicy;
use serde_json::{json, Value};

use crate::manager::{Manager, PluginSetup, PluginsMode};

/// The stand-in plugin program (`crates/test_plugin`). `cargo test --workspace` builds it, a run of
/// one crate does not, so it is built here by name (a no-op when it is current).
pub fn fake_plugin() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let exe = std::env::current_exe().expect("test exe");
        let profile = exe.parent().and_then(Path::parent).expect("target dir");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut cmd = std::process::Command::new(cargo);
        cmd.args(["build", "--quiet", "-p", "fetchbox-test-plugin"]);
        if profile.ends_with("release") {
            cmd.arg("--release");
        }
        assert!(
            cmd.status().expect("cargo").success(),
            "building the stand-in plugin failed"
        );
        let path = profile.join(format!("fake-plugin{}", std::env::consts::EXE_SUFFIX));
        assert!(path.exists(), "no {}", path.display());
        path
    })
    .clone()
}

/// A plugin package: `plugin.json`, and the stand-in program as `bin/fake` when asked.
fn package(manifest: &Value, with_program: bool) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let plain =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    w.start_file("plugin.json", plain).expect("file");
    w.write_all(manifest.to_string().as_bytes()).expect("write");
    if with_program {
        w.start_file("bin/fake", plain.unix_permissions(0o755))
            .expect("file");
        w.write_all(&std::fs::read(fake_plugin()).expect("program"))
            .expect("write");
    }
    w.finish().expect("finish").into_inner()
}

fn manifest(id: &str, extra: Value) -> Value {
    let mut m = json!({"api_version": 1, "id": id, "name": "Demo plugin", "version": "1.0.0"});
    for (k, v) in extra.as_object().expect("object") {
        m[k] = v.clone();
    }
    m
}

/// A plugin that runs the stand-in program.
fn with_process(id: &str) -> Value {
    manifest(
        id,
        json!({
            "process": {"command": "bin/fake", "args": ["echo"]},
            "ui": {"actions": [
                {"id": "ping", "label": "Ping", "kind": "process"},
                {"id": "versions", "label": "Print", "kind": "run", "command": "bin/fake", "args": ["print-json"]}
            ]}
        }),
    )
}

struct Rig {
    mgr: Arc<Manager>,
    root: tempfile::TempDir,
}

impl Rig {
    async fn new() -> Self {
        Self::with_mode(PluginsMode::On).await
    }

    async fn with_mode(mode: PluginsMode) -> Self {
        let root = tempfile::tempdir().expect("tmp");
        let mgr = Self::boot(root.path(), mode, None, "").await;
        Rig { mgr, root }
    }

    /// A manager on the files of `root` (a second call is a restart of the server).
    async fn boot(
        root: &Path,
        mode: PluginsMode,
        github_api: Option<String>,
        index_url: &str,
    ) -> Arc<Manager> {
        let db = fetchbox_storage::open(&root.join("fetchbox.db")).expect("db");
        let policy = UrlPolicy::default();
        let mgr = Manager::start(db, root.join("downloads"), policy)
            .await
            .expect("start");
        mgr.enable_plugins(PluginSetup {
            data_dir: root.join("data"),
            mode,
            policy,
            allow_plain_http: true,
            github_api,
            // The tests never go to the real catalog: empty is none.
            index_url: Some(index_url.to_owned()),
        })
        .await;
        mgr
    }

    fn plugin_dir(&self, id: &str) -> PathBuf {
        self.root.path().join("data/plugins").join(id)
    }

    async fn list(&self) -> PluginList {
        self.mgr.plugins().await.expect("list")
    }

    async fn view(&self, id: &str) -> PluginView {
        self.list()
            .await
            .plugins
            .into_iter()
            .find(|p| p.id == id)
            .expect("plugin")
    }

    /// Stage and install a package; the plugin starts switched off.
    async fn install(&self, manifest: &Value, with_program: bool) -> PluginView {
        let staged = self
            .mgr
            .plugin_stage(package(manifest, with_program))
            .await
            .expect("stage");
        self.mgr
            .plugin_install(staged.token)
            .await
            .expect("install")
    }

    async fn wait_for(&self, id: &str, state: PluginState) -> PluginView {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let v = self.view(id).await;
            if v.state == state {
                return v;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{id} never became {state:?}, it is {:?} ({:?})",
                v.state,
                v.error
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

fn text(s: &str) -> SettingValue {
    SettingValue::Text(s.into())
}

fn values(items: &[(&str, Option<SettingValue>)]) -> BTreeMap<String, Option<SettingValue>> {
    items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

fn invalid(e: Error) -> String {
    match e {
        Error::InvalidInput(m) | Error::InvalidState(m) | Error::Missing(m) => m,
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn a_zip_is_staged_for_review_then_installed_switched_off() {
    let rig = Rig::new().await;
    let m = manifest(
        "demo",
        json!({
            "permissions": {"network": ["example.com"]},
            "hooks": [{"on": ["download.completed"], "action": {"webhook": {"url": "https://hooks.example.org/x"}}}]
        }),
    );
    let staged = rig
        .mgr
        .plugin_stage(package(&m, false))
        .await
        .expect("stage");
    assert_eq!(staged.id, "demo");
    assert_eq!(staged.source, PluginSource::Zip);
    assert_eq!(staged.permissions.network, ["example.com"]);
    assert_eq!(staged.permissions.webhook_hosts, ["hooks.example.org"]);
    assert_eq!(staged.permissions.events, ["download.completed"]);
    assert!(staged.replaces.is_none());
    // Nothing is installed until the user says so.
    assert!(rig.list().await.plugins.is_empty());
    assert!(!rig.plugin_dir("demo").exists());

    let view = rig.mgr.plugin_install(staged.token).await.expect("install");
    assert_eq!(
        view.state,
        PluginState::Disabled,
        "a new plugin starts switched off"
    );
    assert!(!view.enabled);
    assert_eq!(view.source, PluginSource::Zip);
    assert!(rig.plugin_dir("demo").join("plugin.json").is_file());
    assert_eq!(rig.list().await.plugins.len(), 1);
}

#[tokio::test]
async fn a_staged_package_can_be_thrown_away() {
    let rig = Rig::new().await;
    let staged = rig
        .mgr
        .plugin_stage(package(&manifest("demo", json!({})), false))
        .await
        .expect("stage");
    rig.mgr
        .plugin_cancel(staged.token.clone())
        .await
        .expect("cancel");
    let e = rig
        .mgr
        .plugin_install(staged.token)
        .await
        .expect_err("gone");
    assert!(invalid(e).contains("gone"));
    assert!(rig.list().await.plugins.is_empty());
}

#[tokio::test]
async fn bad_packages_are_refused_with_a_reason() {
    let rig = Rig::new().await;
    let e = rig
        .mgr
        .plugin_stage(b"not a zip".to_vec())
        .await
        .expect_err("junk");
    assert!(matches!(e, Error::InvalidInput(_)), "{e:?}");

    let e = rig
        .mgr
        .plugin_stage(package(
            &json!({"api_version": 1, "id": "Bad Id", "name": "x", "version": "1"}),
            false,
        ))
        .await
        .expect_err("id");
    assert!(invalid(e).contains("valid id"));

    let e = rig
        .mgr
        .plugin_stage(package(
            &manifest("demo", json!({"min_fetchbox": "99.0.0"})),
            false,
        ))
        .await
        .expect_err("too new");
    assert!(invalid(e).contains("needs FetchBox 99.0.0"));

    let other = PLATFORMS
        .iter()
        .find(|p| **p != platform_key())
        .expect("a platform");
    let e = rig
        .mgr
        .plugin_stage(package(
            &manifest("demo", json!({"platforms": [other]})),
            false,
        ))
        .await
        .expect_err("platform");
    assert!(invalid(e).contains("does not support"));
    assert!(rig.list().await.plugins.is_empty());
}

#[tokio::test]
async fn switching_on_starts_the_program_and_switching_off_stops_it() {
    let rig = Rig::new().await;
    let mut events = rig.mgr.subscribe();
    rig.install(&with_process("demo"), true).await;
    let on = rig
        .mgr
        .plugin_set_enabled("demo".into(), true)
        .await
        .expect("enable");
    assert!(on.enabled);
    let running = rig.wait_for("demo", PluginState::Running).await;
    assert!(running.provides.process);

    // The program answers an action.
    let answer = rig
        .mgr
        .plugin_action("demo".into(), "ping".into(), BTreeMap::new())
        .await
        .expect("ping");
    assert!(answer.ok);
    assert!(answer.message.expect("message").contains("echo"));
    // A command action prints what the program printed.
    let printed = rig
        .mgr
        .plugin_action("demo".into(), "versions".into(), BTreeMap::new())
        .await
        .expect("run");
    assert!(printed.message.expect("message").contains("A video"));

    let off = rig
        .mgr
        .plugin_set_enabled("demo".into(), false)
        .await
        .expect("disable");
    assert_eq!(off.state, PluginState::Disabled);
    let e = rig
        .mgr
        .plugin_action("demo".into(), "ping".into(), BTreeMap::new())
        .await
        .expect_err("off");
    assert!(invalid(e).contains("not running"));

    // The interface was told along the way.
    let mut states = Vec::new();
    while let Ok(ev) = events.try_recv() {
        if let Event::Plugin { plugin } = ev {
            states.push(plugin.state);
        }
    }
    assert!(states.contains(&PluginState::Preparing), "{states:?}");
    assert!(states.contains(&PluginState::Running), "{states:?}");
    assert_eq!(states.last(), Some(&PluginState::Disabled));
}

#[tokio::test]
async fn a_plugin_without_a_program_is_simply_ready() {
    let rig = Rig::new().await;
    rig.install(&manifest("quiet", json!({})), false).await;
    rig.mgr
        .plugin_set_enabled("quiet".into(), true)
        .await
        .expect("enable");
    rig.wait_for("quiet", PluginState::Ready).await;
}

#[tokio::test]
async fn a_program_that_cannot_start_turns_only_its_own_card_red() {
    let rig = Rig::new().await;
    // The manifest names a program the package does not contain.
    rig.install(&with_process("demo"), false).await;
    rig.mgr
        .plugin_set_enabled("demo".into(), true)
        .await
        .expect("enable");
    let v = rig.wait_for("demo", PluginState::Failed).await;
    assert!(v.error.expect("error").contains("is not there"));
    // Downloads are unaffected: the manager still answers.
    assert!(rig.mgr.list().await.expect("downloads").is_empty());
    // Switching it off clears the failure.
    let off = rig
        .mgr
        .plugin_set_enabled("demo".into(), false)
        .await
        .expect("disable");
    assert_eq!(off.state, PluginState::Disabled);
    assert_eq!(off.error, None);
}

#[tokio::test]
async fn settings_are_checked_stored_and_secrets_never_leave() {
    let rig = Rig::new().await;
    let m = manifest(
        "demo",
        json!({"settings": [
            {"key": "name", "type": "text", "label": "Name", "required": true},
            {"key": "token", "type": "secret", "label": "Token"},
            {"key": "port", "type": "number", "min": 1, "max": 100, "default": 10},
            {"key": "mode", "type": "select", "default": "a", "options": [{"value": "a", "label": "A"}, {"value": "b", "label": "B"}]}
        ]}),
    );
    rig.install(&m, false).await;

    // A required setting without a value keeps the plugin off.
    let e = rig
        .mgr
        .plugin_set_enabled("demo".into(), true)
        .await
        .expect_err("required");
    assert!(invalid(e).contains("set \"Name\" first"));

    let v = rig
        .mgr
        .plugin_save_settings(
            "demo".into(),
            values(&[
                ("name", Some(text("Filip"))),
                ("token", Some(text("s3cret-token"))),
                ("port", Some(SettingValue::Number(42.0))),
            ]),
        )
        .await
        .expect("save");
    let field = |v: &PluginView, k: &str| {
        v.settings
            .iter()
            .find(|f| f.field.key == k)
            .expect("field")
            .clone()
    };
    assert_eq!(field(&v, "name").value, Some(text("Filip")));
    assert_eq!(field(&v, "port").value, Some(SettingValue::Number(42.0)));
    assert_eq!(
        field(&v, "mode").value,
        Some(text("a")),
        "the default shows when nothing is stored"
    );
    assert!(!field(&v, "mode").has_value);
    let token = field(&v, "token");
    assert!(
        token.has_value && token.value.is_none(),
        "a secret is never sent back"
    );
    // Not in the list, not in an event, not in JSON of any view.
    let json = serde_json::to_string(&rig.list().await).expect("json");
    assert!(!json.contains("s3cret-token"));

    // Wrong values are refused and change nothing.
    for (k, bad) in [
        ("port", SettingValue::Number(500.0)),
        ("port", text("x")),
        ("mode", text("z")),
        ("name", SettingValue::Bool(true)),
        ("nope", text("x")),
    ] {
        let e = rig
            .mgr
            .plugin_save_settings("demo".into(), values(&[(k, Some(bad))]))
            .await
            .expect_err(k);
        assert!(matches!(e, Error::InvalidInput(_)), "{k}: {e:?}");
    }
    assert_eq!(
        field(&rig.view("demo").await, "port").value,
        Some(SettingValue::Number(42.0))
    );

    // A key can be cleared; one that is left out stays.
    let v = rig
        .mgr
        .plugin_save_settings("demo".into(), values(&[("token", None)]))
        .await
        .expect("clear");
    assert!(!field(&v, "token").has_value);
    assert_eq!(field(&v, "name").value, Some(text("Filip")));

    // Now it can be switched on.
    rig.mgr
        .plugin_set_enabled("demo".into(), true)
        .await
        .expect("enable");
    rig.wait_for("demo", PluginState::Ready).await;
}

#[tokio::test]
async fn files_that_change_on_disk_must_be_approved_again() {
    let rig = Rig::new().await;
    rig.install(&with_process("demo"), true).await;
    rig.mgr
        .plugin_set_enabled("demo".into(), true)
        .await
        .expect("enable");
    rig.wait_for("demo", PluginState::Running).await;

    // Somebody edits plugin.json by hand.
    let path = rig.plugin_dir("demo").join("plugin.json");
    let mut m: Value = serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    m["version"] = json!("1.0.1");
    m["permissions"] = json!({"network": ["evil.example"]});
    std::fs::write(&path, m.to_string()).expect("write");

    let list = rig.mgr.plugin_rescan().await.expect("rescan");
    let v = &list.plugins[0];
    assert_eq!(v.state, PluginState::NeedsReview);
    assert!(v.enabled, "the choice is remembered");
    assert_eq!(
        v.permissions.network,
        ["evil.example"],
        "the review shows what is on disk now"
    );
    let e = rig
        .mgr
        .plugin_action("demo".into(), "ping".into(), BTreeMap::new())
        .await
        .expect_err("stopped");
    assert!(invalid(e).contains("not running"), "it no longer runs");
    let e = rig
        .mgr
        .plugin_set_enabled("demo".into(), true)
        .await
        .expect_err("review");
    assert!(invalid(e).contains("approve it first"));

    // Approving needs the hash the user was shown.
    let e = rig
        .mgr
        .plugin_approve("demo".into(), "0".repeat(64))
        .await
        .expect_err("stale");
    assert!(invalid(e).contains("changed since you looked"));
    let hash = v.manifest_hash.clone();
    rig.mgr
        .plugin_approve("demo".into(), hash)
        .await
        .expect("approve");
    rig.wait_for("demo", PluginState::Running).await;
}

#[tokio::test]
async fn a_folder_put_in_the_plugins_directory_waits_for_approval() {
    let rig = Rig::new().await;
    let dir = rig.plugin_dir("dropped");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join("plugin.json"),
        manifest("dropped", json!({})).to_string(),
    )
    .expect("write");
    // A folder whose name is not its plugin's id, and one with a broken manifest.
    let wrong = rig.plugin_dir("wrong-name");
    std::fs::create_dir_all(&wrong).expect("dir");
    std::fs::write(
        wrong.join("plugin.json"),
        manifest("other", json!({})).to_string(),
    )
    .expect("write");
    let broken = rig.plugin_dir("broken");
    std::fs::create_dir_all(&broken).expect("dir");
    std::fs::write(broken.join("plugin.json"), "{").expect("write");

    let list = rig.mgr.plugin_rescan().await.expect("rescan");
    let by = |id: &str| {
        list.plugins
            .iter()
            .find(|p| p.id == id)
            .expect("plugin")
            .clone()
    };
    let dropped = by("dropped");
    assert_eq!(dropped.state, PluginState::NeedsReview);
    assert_eq!(dropped.source, PluginSource::Folder);
    assert!(!dropped.enabled);
    assert_eq!(by("wrong-name").state, PluginState::Failed);
    assert!(by("wrong-name").error.expect("error").contains("is called"));
    assert_eq!(by("broken").state, PluginState::Failed);

    let e = rig
        .mgr
        .plugin_set_enabled("dropped".into(), true)
        .await
        .expect_err("review");
    assert!(invalid(e).contains("approve it first"));
    rig.mgr
        .plugin_approve("dropped".into(), dropped.manifest_hash)
        .await
        .expect("approve");
    assert_eq!(rig.view("dropped").await.state, PluginState::Disabled);
    rig.mgr
        .plugin_set_enabled("dropped".into(), true)
        .await
        .expect("enable");
    rig.wait_for("dropped", PluginState::Ready).await;

    // A folder that is deleted takes its plugin with it at the next scan.
    std::fs::remove_dir_all(&dir).expect("remove");
    let list = rig.mgr.plugin_rescan().await.expect("rescan");
    assert!(list.plugins.iter().all(|p| p.id != "dropped"));
}

#[tokio::test]
async fn an_update_replaces_the_plugin_and_keeps_its_choices() {
    let rig = Rig::new().await;
    let m = manifest(
        "demo",
        json!({"settings": [{"key": "name", "type": "text"}]}),
    );
    rig.install(&m, false).await;
    rig.mgr
        .plugin_save_settings("demo".into(), values(&[("name", Some(text("kept")))]))
        .await
        .expect("save");
    rig.mgr
        .plugin_set_enabled("demo".into(), true)
        .await
        .expect("enable");
    rig.wait_for("demo", PluginState::Ready).await;

    let mut newer = m.clone();
    newer["version"] = json!("2.0.0");
    newer["permissions"] = json!({"network": ["new.example"]});
    let staged = rig
        .mgr
        .plugin_stage(package(&newer, false))
        .await
        .expect("stage");
    let old = staged.replaces.clone().expect("replaces");
    assert_eq!(old.version, "1.0.0");
    assert!(old.permissions.network.is_empty());
    assert_eq!(
        staged.permissions.network,
        ["new.example"],
        "the review can show what changed"
    );
    let v = rig.mgr.plugin_install(staged.token).await.expect("install");
    assert_eq!(v.version, "2.0.0");
    assert!(v.enabled, "an update does not switch it off");
    rig.wait_for("demo", PluginState::Ready).await;
    assert_eq!(v.settings[0].value, Some(text("kept")));
}

#[tokio::test]
async fn removing_a_plugin_forgets_it_and_can_keep_its_data() {
    let rig = Rig::new().await;
    let mut events = rig.mgr.subscribe();
    rig.install(
        &manifest(
            "demo",
            json!({"settings": [{"key": "name", "type": "text"}]}),
        ),
        false,
    )
    .await;
    rig.mgr
        .plugin_save_settings("demo".into(), values(&[("name", Some(text("x")))]))
        .await
        .expect("save");
    let data = rig.root.path().join("data/plugin-data/demo");
    std::fs::create_dir_all(&data).expect("data");
    std::fs::write(data.join("note"), "x").expect("write");

    rig.mgr
        .plugin_remove("demo".into(), true)
        .await
        .expect("remove");
    assert!(rig.list().await.plugins.is_empty());
    assert!(!rig.plugin_dir("demo").exists());
    assert!(data.join("note").exists(), "kept on request");
    let mut removed = false;
    while let Ok(ev) = events.try_recv() {
        removed |= matches!(ev, Event::PluginRemoved { id } if id == "demo");
    }
    assert!(removed);
    let e = rig
        .mgr
        .plugin_remove("demo".into(), false)
        .await
        .expect_err("gone");
    assert!(invalid(e).contains("no plugin"));

    // Installed again, its settings are gone with the old install, and its data goes this time.
    rig.install(
        &manifest(
            "demo",
            json!({"settings": [{"key": "name", "type": "text"}]}),
        ),
        false,
    )
    .await;
    assert_eq!(rig.view("demo").await.settings[0].value, None);
    rig.mgr
        .plugin_remove("demo".into(), false)
        .await
        .expect("remove");
    assert!(!data.exists());
}

#[tokio::test]
async fn plugins_stay_off_where_they_must() {
    for (mode, reason) in [(PluginsMode::NoAuth, "no_auth"), (PluginsMode::Off, "off")] {
        let rig = Rig::with_mode(mode).await;
        let list = rig.list().await;
        assert!(!list.available);
        assert_eq!(list.reason.as_deref(), Some(reason));
        assert!(list.plugins.is_empty());
        let e = rig
            .mgr
            .plugin_stage(package(&manifest("demo", json!({})), false))
            .await
            .expect_err("off");
        assert!(matches!(e, Error::InvalidState(_)), "{e:?}");
        let e = rig
            .mgr
            .plugin_set_enabled("demo".into(), true)
            .await
            .expect_err("off");
        assert!(matches!(e, Error::InvalidState(_)), "{e:?}");
    }
}

#[tokio::test]
async fn an_enabled_plugin_runs_again_after_a_restart_of_the_server() {
    let rig = Rig::new().await;
    rig.install(&with_process("demo"), true).await;
    rig.mgr
        .plugin_set_enabled("demo".into(), true)
        .await
        .expect("enable");
    rig.wait_for("demo", PluginState::Running).await;
    rig.mgr.stop_plugins().await;

    let again = Rig {
        mgr: Rig::boot(rig.root.path(), PluginsMode::On, None, "").await,
        root: rig.root,
    };
    let v = again.view("demo").await;
    assert!(v.enabled);
    again.wait_for("demo", PluginState::Running).await;
    again.mgr.stop_plugins().await;
}

/// A tiny GitHub: the releases API of one repository and the files of its latest release.
async fn fake_github(asset: &'static [u8], sums: String) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let release = {
        let base = base.clone();
        let sums_len = sums.len();
        move || {
            let doc = json!({
                "tag_name": "v1.2.3",
                "assets": [
                    {"name": "tool_asset", "size": asset.len(), "browser_download_url": format!("{base}/dl/tool")},
                    {"name": "SUMS", "size": sums_len, "browser_download_url": format!("{base}/dl/sums")},
                ]
            });
            async move { doc.to_string() }
        }
    };
    let app = axum::Router::new()
        .route("/repos/o/r/releases/latest", axum::routing::get(release))
        .route("/dl/tool", axum::routing::get(move || async move { asset }))
        .route(
            "/dl/sums",
            axum::routing::get(move || {
                let s = sums.clone();
                async move { s }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await });
    base
}

fn sha(bytes: &[u8]) -> String {
    fetchbox_plugins::install::sha256_hex(bytes)
}

fn needing() -> Value {
    manifest(
        "needy",
        json!({"platforms": [platform_key()], "dependencies": [{
            "id": "tool",
            "github": {"repo": "o/r", "assets": {platform_key(): "tool_asset"}, "sha256_asset": "SUMS"}
        }]}),
    )
}

async fn rig_with_github(api: String) -> Rig {
    let root = tempfile::tempdir().expect("tmp");
    let mgr = Rig::boot(root.path(), PluginsMode::On, Some(api), "").await;
    Rig { mgr, root }
}

#[tokio::test]
async fn a_dependency_is_fetched_when_the_plugin_is_switched_on() {
    const BODY: &[u8] = b"a program";
    let api = fake_github(BODY, format!("{}  tool_asset\n", sha(BODY))).await;
    let rig = rig_with_github(api).await;
    let mut events = rig.mgr.subscribe();
    rig.install(&needing(), false).await;
    assert!(!rig.view("needy").await.dependencies[0].installed);
    rig.mgr
        .plugin_set_enabled("needy".into(), true)
        .await
        .expect("enable");
    let v = rig.wait_for("needy", PluginState::Ready).await;
    assert!(v.dependencies[0].installed);
    assert_eq!(v.dependencies[0].version.as_deref(), Some("v1.2.3"));
    assert!(rig
        .root
        .path()
        .join("data/plugin-data/needy/deps/tool/tool")
        .is_file());

    let mut progress = false;
    while let Ok(ev) = events.try_recv() {
        if let Event::Plugin { plugin } = ev {
            progress |= plugin.progress.is_some_and(|p| p.dependency == "tool");
        }
    }
    assert!(progress, "the interface saw the download");
}

#[tokio::test]
async fn a_dependency_with_a_wrong_checksum_fails_the_plugin_not_the_server() {
    let api = fake_github(b"a program", format!("{}  tool_asset\n", "0".repeat(64))).await;
    let rig = rig_with_github(api).await;
    rig.install(&needing(), false).await;
    rig.mgr
        .plugin_set_enabled("needy".into(), true)
        .await
        .expect("enable");
    let v = rig.wait_for("needy", PluginState::Failed).await;
    assert!(v.error.expect("error").contains("does not match"));
    assert!(!v.dependencies[0].installed);
    assert!(
        v.enabled,
        "it stays switched on: fixing the cause and restarting is enough"
    );
    assert!(
        rig.mgr.list().await.expect("downloads").is_empty(),
        "downloads are unaffected"
    );
}

#[tokio::test]
async fn a_webhook_action_posts_to_the_address_in_the_settings() {
    let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let sink = seen.clone();
    let app = axum::Router::new().route(
        "/hook",
        axum::routing::post(move |body: String| {
            let sink = sink.clone();
            async move {
                sink.lock().expect("lock").push(body);
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    tokio::spawn(async move { axum::serve(listener, app).await });

    let rig = Rig::new().await;
    rig.install(
        &manifest("notify", json!({
            "settings": [{"key": "url", "type": "text", "required": true}],
            "ui": {"actions": [{"id": "test", "label": "Test", "kind": "webhook", "webhook": {"url": "${setting:url}"}}]}
        })),
        false,
    )
    .await;
    rig.mgr
        .plugin_save_settings(
            "notify".into(),
            values(&[("url", Some(text(&format!("{base}/hook"))))]),
        )
        .await
        .expect("save");
    rig.mgr
        .plugin_set_enabled("notify".into(), true)
        .await
        .expect("enable");
    rig.wait_for("notify", PluginState::Ready).await;
    let input = BTreeMap::from([("message".to_owned(), text("hello"))]);
    let r = rig
        .mgr
        .plugin_action("notify".into(), "test".into(), input)
        .await
        .expect("action");
    assert!(r.message.expect("message").contains("200"));
    let body: Value = serde_json::from_str(&seen.lock().expect("lock")[0]).expect("json");
    assert_eq!(body["plugin"], "notify");
    assert_eq!(body["input"]["message"], "hello");

    let e = rig
        .mgr
        .plugin_action("notify".into(), "nope".into(), BTreeMap::new())
        .await
        .expect_err("unknown");
    assert!(invalid(e).contains("not an action"));
}

// ---- over HTTP --------------------------------------------------------------------

async fn serve(mgr: Arc<Manager>, auth: Arc<fetchbox_api::Auth>) -> String {
    let app = fetchbox_api::router(mgr, auth);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let base = format!("http://{}/api", listener.local_addr().expect("addr"));
    tokio::spawn(async move { axum::serve(listener, app).await });
    base
}

/// `reqwest` is built without its `json` feature here: a JSON body is a string and a header.
trait JsonBody {
    fn jbody(self, v: Value) -> Self;
}

impl JsonBody for reqwest::RequestBuilder {
    fn jbody(self, v: Value) -> Self {
        self.header("content-type", "application/json")
            .body(v.to_string())
    }
}

async fn answer(r: reqwest::Response) -> (u16, Value) {
    let status = r.status().as_u16();
    let text = r.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_plugin_routes_need_a_session() {
    let rig = Rig::new().await;
    let api = serve(
        rig.mgr.clone(),
        fetchbox_api::Auth::with_password("correct horse"),
    )
    .await;
    let http = reqwest::Client::new();
    for (method, path) in [
        ("GET", "/plugins"),
        ("POST", "/plugin-rescan"),
        ("POST", "/plugin-staging"),
        ("DELETE", "/plugin-staging/abc"),
        ("PATCH", "/plugins/demo"),
        ("DELETE", "/plugins/demo"),
        ("PUT", "/plugins/demo/settings"),
        ("POST", "/plugins/demo/actions/x"),
        ("GET", "/plugins/demo/logs"),
    ] {
        let r = http
            .request(method.parse().expect("method"), format!("{api}{path}"))
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .expect("request");
        assert_eq!(r.status().as_u16(), 401, "{method} {path}");
    }
}

#[tokio::test]
async fn a_plugin_can_be_installed_used_and_removed_over_http() {
    let rig = Rig::new().await;
    let api = serve(rig.mgr.clone(), fetchbox_api::Auth::disabled()).await;
    let http = reqwest::Client::new();
    let m = manifest(
        "demo",
        json!({
            "settings": [{"key": "name", "type": "text"}, {"key": "token", "type": "secret"}],
            "ui": {"actions": [{"id": "open", "label": "Open", "kind": "open_dialog", "dialog": "d"}],
                   "dialogs": [{"id": "d", "title": "D"}]}
        }),
    );

    // The ZIP must say it is one.
    let r = http
        .post(format!("{api}/plugin-staging"))
        .header("content-type", "text/plain")
        .body(package(&m, false))
        .send()
        .await
        .expect("post");
    let (status, body) = answer(r).await;
    assert_eq!(status, 400);
    assert!(body["error"]
        .as_str()
        .expect("error")
        .contains("application/zip"));

    let r = http
        .post(format!("{api}/plugin-staging"))
        .header("content-type", "application/zip")
        .body(package(&m, false))
        .send()
        .await
        .expect("post");
    let (status, staged) = answer(r).await;
    assert_eq!(status, 201, "{staged}");
    assert_eq!(staged["id"], "demo");
    let token = staged["token"].as_str().expect("token").to_owned();

    let (status, view) = answer(
        http.post(format!("{api}/plugin-staging/{token}/install"))
            .send()
            .await
            .expect("post"),
    )
    .await;
    assert_eq!(status, 201, "{view}");
    assert_eq!(view["state"], "disabled");

    let (status, list) = answer(
        http.get(format!("{api}/plugins"))
            .send()
            .await
            .expect("get"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(list["available"], true);
    assert_eq!(list["plugins"][0]["id"], "demo");

    // JSON bodies must be JSON.
    let r = http
        .patch(format!("{api}/plugins/demo"))
        .header("content-type", "text/plain")
        .body(r#"{"enabled":true}"#)
        .send()
        .await
        .expect("patch");
    assert_eq!(r.status().as_u16(), 415);
    let r = http
        .patch(format!("{api}/plugins/demo"))
        .jbody(json!({"enabled": true}))
        .send()
        .await
        .expect("patch");
    let (status, view) = answer(r).await;
    assert_eq!(status, 200, "{view}");
    rig.wait_for("demo", PluginState::Ready).await;

    // Settings: store, then clear; the secret is never in an answer.
    let r = http
        .put(format!("{api}/plugins/demo/settings"))
        .jbody(json!({"values": {"name": "Filip", "token": "s3cret-value"}}))
        .send()
        .await
        .expect("put");
    let (status, view) = answer(r).await;
    assert_eq!(status, 200, "{view}");
    assert!(!view.to_string().contains("s3cret-value"));
    assert_eq!(view["settings"][1]["has_value"], true);
    let r = http
        .put(format!("{api}/plugins/demo/settings"))
        .jbody(json!({"clear": ["name"]}))
        .send()
        .await
        .expect("put");
    let (_, view) = answer(r).await;
    assert_eq!(view["settings"][0]["has_value"], false);
    assert_eq!(view["settings"][1]["has_value"], true, "left out stays");
    let r = http
        .put(format!("{api}/plugins/demo/settings"))
        .jbody(json!({"values": {"ghost": "x"}}))
        .send()
        .await
        .expect("put");
    assert_eq!(r.status().as_u16(), 400);

    // An action: pressed by name, with input.
    let r = http
        .post(format!("{api}/plugins/demo/actions/open"))
        .jbody(json!({}))
        .send()
        .await
        .expect("post");
    let (status, result) = answer(r).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["open_dialog"], "d");
    let r = http
        .post(format!("{api}/plugins/demo/actions/nope"))
        .jbody(json!({}))
        .send()
        .await
        .expect("post");
    assert_eq!(r.status().as_u16(), 404);

    let (status, logs) = answer(
        http.get(format!("{api}/plugins/demo/logs"))
            .send()
            .await
            .expect("get"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(logs["lines"], json!([]));

    let r = http
        .delete(format!("{api}/plugins/demo"))
        .send()
        .await
        .expect("delete");
    assert_eq!(r.status().as_u16(), 204);
    let (_, list) = answer(
        http.get(format!("{api}/plugins"))
            .send()
            .await
            .expect("get"),
    )
    .await;
    assert_eq!(list["plugins"], json!([]));
    assert_eq!(
        http.delete(format!("{api}/plugins/demo"))
            .send()
            .await
            .expect("delete")
            .status()
            .as_u16(),
        404
    );
}

#[tokio::test]
async fn an_oversized_upload_is_refused_before_it_is_read() {
    let rig = Rig::new().await;
    let api = serve(rig.mgr.clone(), fetchbox_api::Auth::disabled()).await;
    let big = vec![0u8; fetchbox_core::plugin::MAX_PLUGIN_ZIP + 4096];
    let r = reqwest::Client::new()
        .post(format!("{api}/plugin-staging"))
        .header("content-type", "application/zip")
        .body(big)
        .send()
        .await
        .expect("post");
    assert_eq!(r.status().as_u16(), 413);
}

#[tokio::test]
async fn a_staged_package_can_be_cancelled_over_http() {
    let rig = Rig::new().await;
    let api = serve(rig.mgr.clone(), fetchbox_api::Auth::disabled()).await;
    let http = reqwest::Client::new();
    let r = http
        .post(format!("{api}/plugin-staging"))
        .header("content-type", "application/zip")
        .body(package(&manifest("demo", json!({})), false))
        .send()
        .await
        .expect("post");
    let (_, staged) = answer(r).await;
    let token = staged["token"].as_str().expect("token");
    assert_eq!(
        http.delete(format!("{api}/plugin-staging/{token}"))
            .send()
            .await
            .expect("delete")
            .status()
            .as_u16(),
        204
    );
    assert_eq!(
        http.post(format!("{api}/plugin-staging/{token}/install"))
            .send()
            .await
            .expect("post")
            .status()
            .as_u16(),
        404
    );
    // Nothing that is not a token can name a path.
    assert_eq!(
        http.post(format!("{api}/plugin-staging/..%2F..%2Fetc/install"))
            .send()
            .await
            .expect("post")
            .status()
            .as_u16(),
        404
    );
}

// ---- the store --------------------------------------------------------------------

#[tokio::test]
async fn the_store_offers_the_plugins_that_ship_with_fetchbox_offline() {
    let rig = Rig::new().await;
    let list = rig.mgr.plugin_store(false).await.expect("store");
    assert!(!list.remote, "no remote catalog is configured");
    assert!(list.error.is_none());
    let e = list
        .entries
        .iter()
        .find(|e| e.id == "webhook-notify")
        .expect("bundled");
    assert!(e.bundled && e.download.is_none());
    assert_eq!(
        (e.installed.as_deref(), e.update_available, e.unsupported),
        (None, false, false)
    );
}

#[tokio::test]
async fn a_bundled_plugin_is_installed_through_the_same_review() {
    let rig = Rig::new().await;
    let staged = rig
        .mgr
        .plugin_stage_store("webhook-notify".into())
        .await
        .expect("stage");
    assert_eq!(staged.source, PluginSource::Bundled);
    assert_eq!(staged.permissions.webhook_hosts, ["{setting:url}"]);
    assert_eq!(
        staged.permissions.events,
        ["download.completed", "download.failed"]
    );
    assert!(
        rig.list().await.plugins.is_empty(),
        "staging installs nothing"
    );
    let v = rig.mgr.plugin_install(staged.token).await.expect("install");
    assert_eq!(
        (v.source, v.state),
        (PluginSource::Bundled, PluginState::Disabled)
    );
    assert_eq!(v.settings.len(), 2);
    let list = rig.mgr.plugin_store(false).await.expect("store");
    let e = list
        .entries
        .iter()
        .find(|e| e.id == "webhook-notify")
        .expect("entry");
    assert_eq!(e.installed.as_deref(), Some("1.0.0"));
    assert!(!e.update_available);
    let e = rig
        .mgr
        .plugin_stage_store("nope".into())
        .await
        .expect_err("unknown");
    assert!(matches!(e, Error::Missing(_)), "{e:?}");
    let e = rig
        .mgr
        .plugin_stage_store("../etc".into())
        .await
        .expect_err("bad id");
    assert!(matches!(e, Error::Missing(_)), "{e:?}");
}

/// A catalog server: `/index.json` and the ZIPs it lists.
async fn catalog(plugins: Vec<(Value, Vec<u8>)>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let mut index = Vec::new();
    let mut app = axum::Router::new();
    for (i, (mut entry, zip)) in plugins.into_iter().enumerate() {
        let zip: &'static [u8] = Box::leak(zip.into_boxed_slice());
        if entry["download"].is_null() {
            entry["download"] =
                json!({"url": format!("{base}/{i}.zip"), "sha256": sha(zip), "size": zip.len()});
        } else {
            entry["download"]["url"] = json!(format!("{base}/{i}.zip"));
        }
        index.push(entry);
        app = app.route(
            &format!("/{i}.zip"),
            axum::routing::get(move || async move { zip }),
        );
    }
    let doc = json!({"format": 1, "plugins": index}).to_string();
    app = app.route(
        "/index.json",
        axum::routing::get(move || {
            let d = doc.clone();
            async move { d }
        }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("{base}/index.json")
}

fn listed(id: &str, version: &str) -> Value {
    json!({"id": id, "name": format!("Plugin {id}"), "version": version, "description": "From the catalog",
           "download": null})
}

async fn rig_with_catalog(index: &str) -> Rig {
    let root = tempfile::tempdir().expect("tmp");
    let mgr = Rig::boot(root.path(), PluginsMode::On, None, index).await;
    Rig { mgr, root }
}

#[tokio::test]
async fn the_remote_catalog_adds_plugins_and_the_newer_version_wins() {
    let mut newer = manifest("webhook-notify", json!({"settings": []}));
    newer["version"] = json!("2.0.0");
    let index = catalog(vec![
        (
            listed("remote-one", "1.0.0"),
            package(&manifest("remote-one", json!({})), false),
        ),
        (listed("webhook-notify", "2.0.0"), package(&newer, false)),
    ])
    .await;
    let rig = rig_with_catalog(&index).await;
    let list = rig.mgr.plugin_store(false).await.expect("store");
    assert!(list.remote && list.fetched_at.is_some() && list.error.is_none());
    let find = |id: &str| list.entries.iter().find(|e| e.id == id).expect(id).clone();
    assert!(!find("remote-one").bundled);
    let hook = find("webhook-notify");
    assert_eq!(hook.version, "2.0.0");
    assert!(
        hook.download.is_some() && !hook.bundled,
        "the catalog's newer version replaces the bundled one"
    );
}

#[tokio::test]
async fn a_catalog_plugin_is_downloaded_checked_and_installed() {
    let index = catalog(vec![(
        listed("remote-one", "1.0.0"),
        package(&manifest("remote-one", json!({})), false),
    )])
    .await;
    let rig = rig_with_catalog(&index).await;
    let staged = rig
        .mgr
        .plugin_stage_store("remote-one".into())
        .await
        .expect("stage");
    assert_eq!(staged.source, PluginSource::Store);
    let v = rig.mgr.plugin_install(staged.token).await.expect("install");
    assert_eq!(v.source, PluginSource::Store);
    assert!(v.update_available.is_none());
    let list = rig.mgr.plugin_store(false).await.expect("store");
    assert_eq!(
        list.entries
            .iter()
            .find(|e| e.id == "remote-one")
            .expect("entry")
            .installed
            .as_deref(),
        Some("1.0.0")
    );
}

#[tokio::test]
async fn an_installed_plugin_learns_about_a_newer_version_in_the_store() {
    let rig_dir = tempfile::tempdir().expect("tmp");
    // First: only the bundled webhook-notify (1.0.0) is installed.
    let offline = Rig::boot(rig_dir.path(), PluginsMode::On, None, "").await;
    let staged = offline
        .plugin_stage_store("webhook-notify".into())
        .await
        .expect("stage");
    offline.plugin_install(staged.token).await.expect("install");
    offline.stop_plugins().await;
    // Then the server starts with a catalog that has 2.0.0.
    let mut newer = manifest("webhook-notify", json!({}));
    newer["version"] = json!("2.0.0");
    let index = catalog(vec![(
        listed("webhook-notify", "2.0.0"),
        package(&newer, false),
    )])
    .await;
    let online = Rig::boot(rig_dir.path(), PluginsMode::On, None, &index).await;
    let list = online.plugin_store(false).await.expect("store");
    let e = list
        .entries
        .iter()
        .find(|e| e.id == "webhook-notify")
        .expect("entry");
    assert_eq!(
        (e.installed.as_deref(), e.update_available),
        (Some("1.0.0"), true)
    );
    let plugins = online.plugins().await.expect("list");
    assert_eq!(
        plugins.plugins[0].update_available.as_deref(),
        Some("2.0.0")
    );

    // Updating is a review like any install: it shows what the old version could do.
    let staged = online
        .plugin_stage_store("webhook-notify".into())
        .await
        .expect("stage");
    assert_eq!(staged.replaces.expect("replaces").version, "1.0.0");
    assert_eq!(staged.version, "2.0.0");
}

#[tokio::test]
async fn a_download_that_does_not_match_the_catalog_is_refused() {
    let zip = package(&manifest("remote-one", json!({})), false);
    let mut entry = listed("remote-one", "1.0.0");
    entry["download"] = json!({"url": "x", "sha256": "0".repeat(64)});
    let index = catalog(vec![(entry, zip)]).await;
    let rig = rig_with_catalog(&index).await;
    let e = rig
        .mgr
        .plugin_stage_store("remote-one".into())
        .await
        .expect_err("mismatch");
    assert!(
        matches!(&e, Error::Engine(m) if m.contains("does not match")),
        "{e:?}"
    );
    // Nothing was left lying around.
    let staging = rig.root.path().join("data/plugins/.staging");
    assert_eq!(
        std::fs::read_dir(staging).map(|d| d.count()).unwrap_or(0),
        0
    );
}

#[tokio::test]
async fn a_package_that_is_not_the_plugin_the_catalog_named_is_refused() {
    let index = catalog(vec![(
        listed("alpha", "1.0.0"),
        package(&manifest("beta", json!({})), false),
    )])
    .await;
    let rig = rig_with_catalog(&index).await;
    let e = rig
        .mgr
        .plugin_stage_store("alpha".into())
        .await
        .expect_err("mislabeled");
    assert!(invalid(e).contains("not \"alpha\""));
    assert!(rig.list().await.plugins.is_empty());
}

#[tokio::test]
async fn a_catalog_entry_that_cannot_run_here_is_marked_and_refused() {
    let mut entry = listed("too-new", "1.0.0");
    entry["min_fetchbox"] = json!("99.0.0");
    let index = catalog(vec![(
        entry,
        package(&manifest("too-new", json!({})), false),
    )])
    .await;
    let rig = rig_with_catalog(&index).await;
    let list = rig.mgr.plugin_store(false).await.expect("store");
    assert!(
        list.entries
            .iter()
            .find(|e| e.id == "too-new")
            .expect("entry")
            .unsupported
    );
    let e = rig
        .mgr
        .plugin_stage_store("too-new".into())
        .await
        .expect_err("unsupported");
    assert!(invalid(e).contains("cannot be used here"));
}

#[tokio::test]
async fn an_unreachable_catalog_still_lists_the_bundled_plugins() {
    let rig = rig_with_catalog("http://127.0.0.1:9/index.json").await;
    let list = rig.mgr.plugin_store(false).await.expect("store");
    assert!(list.remote);
    assert!(list.error.is_some(), "the reason is shown");
    assert!(list.entries.iter().any(|e| e.id == "webhook-notify"));
}

#[tokio::test]
async fn the_store_routes_list_stage_and_refuse() {
    let rig = Rig::new().await;
    let api = serve(rig.mgr.clone(), fetchbox_api::Auth::disabled()).await;
    let http = reqwest::Client::new();
    let (status, store) = answer(
        http.get(format!("{api}/plugin-store"))
            .send()
            .await
            .expect("get"),
    )
    .await;
    assert_eq!(status, 200, "{store}");
    assert_eq!(store["remote"], false);
    assert_eq!(store["entries"][0]["id"], "webhook-notify");

    let (status, _) = answer(
        http.post(format!("{api}/plugin-store/refresh"))
            .send()
            .await
            .expect("post"),
    )
    .await;
    assert_eq!(status, 200);

    let r = http
        .post(format!("{api}/plugin-staging/store"))
        .jbody(json!({"id": "webhook-notify"}))
        .send()
        .await
        .expect("post");
    let (status, staged) = answer(r).await;
    assert_eq!(status, 201, "{staged}");
    assert_eq!(staged["source"], "bundled");
    let token = staged["token"].as_str().expect("token");
    let r = http
        .post(format!("{api}/plugin-staging/{token}/install"))
        .send()
        .await
        .expect("post");
    assert_eq!(r.status().as_u16(), 201);

    let r = http
        .post(format!("{api}/plugin-staging/store"))
        .jbody(json!({"id": "nope"}))
        .send()
        .await
        .expect("post");
    assert_eq!(r.status().as_u16(), 404);
}

// ---- resolvers --------------------------------------------------------------------

use fetchbox_core::{AddRequest, Category, OnConflict, UrlKind, Via};

fn add_request(url: &str, via: Option<Via>, filename: Option<&str>) -> AddRequest {
    AddRequest {
        url: url.into(),
        filename: filename.map(str::to_owned),
        category: None,
        on_conflict: OnConflict::Reject,
        ignore_space: false,
        start_at: None,
        start_now: false,
        checksum: None,
        disk: None,
        credentials: None,
        via,
    }
}

fn via(plugin: &str, resolver: &str, choice: &str) -> Via {
    Via {
        plugin: plugin.into(),
        resolver: resolver.into(),
        reference: Some(choice.into()),
    }
}

/// A plugin with two resolvers, both run by the stand-in program: `site` finds addresses for the
/// engine to download, `media` makes files itself, in three presets (one needs ffmpeg).
fn resolving() -> Value {
    manifest(
        "finder",
        json!({
            "platforms": [platform_key()],
            "settings": [{"key": "extra", "type": "textarea"}],
            "dependencies": [{
                "id": "ffmpeg", "label": "ffmpeg", "optional": true,
                "platforms": {platform_key(): {"url": "https://example.com/ffmpeg", "sha256": "0".repeat(64)}}
            }],
            "resolvers": [
                {
                    "id": "site", "match": ["https://site.example/*"],
                    "probe": {"command": "bin/fake", "args": ["print-json", "${url}"]},
                    "result": {"items": "/entries", "item": {"name": "/title", "url": "/url"}}
                },
                {
                    "id": "media", "match": ["https://video.example/*", "${setting:extra}"],
                    "probe": {"command": "bin/fake", "args": ["print-single"]},
                    "result": {"single": {"name": "/title", "size": "/filesize"}},
                    "choices": [
                        {"id": "best", "label": "Best quality", "category": "video", "ext": "mp4", "requires": ["ffmpeg"], "args": []},
                        {"id": "single", "label": "Best single file", "category": "video", "args": []},
                        {"id": "audio", "label": "Audio only", "category": "music", "ext": "m4a", "args": []}
                    ],
                    "run": {"command": "bin/fake", "args": ["print-json"]}
                }
            ]
        }),
    )
}

async fn rig_with_resolvers() -> Rig {
    let rig = Rig::new().await;
    rig.install(&resolving(), true).await;
    rig.mgr
        .plugin_set_enabled("finder".into(), true)
        .await
        .expect("enable");
    rig.wait_for("finder", PluginState::Ready).await;
    rig
}

#[tokio::test]
async fn a_resolver_that_finds_addresses_lists_them_for_the_engine() {
    let rig = rig_with_resolvers().await;
    let info = rig
        .mgr
        .probe("https://site.example/gallery", None)
        .await
        .expect("probe");
    assert_eq!(info.kind, UrlKind::Listing);
    let names: Vec<_> = info
        .entries
        .iter()
        .map(|e| (e.name.as_str(), e.url.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("One", "https://v.example/1"),
            ("Two", "https://v.example/2")
        ]
    );
    assert!(
        info.entries.iter().all(|e| e.via.is_none()),
        "the engine downloads these as they are"
    );
}

#[tokio::test]
async fn a_resolver_that_makes_files_offers_one_entry_per_preset_that_can_run() {
    let rig = rig_with_resolvers().await;
    let info = rig
        .mgr
        .probe("https://video.example/watch?v=1", None)
        .await
        .expect("probe");
    assert_eq!(info.kind, UrlKind::Listing);
    assert_eq!(info.filename, "A video");
    // "Best quality" needs ffmpeg, which is not installed: it is not offered.
    let got: Vec<_> = info
        .entries
        .iter()
        .map(|e| {
            (
                e.name.as_str(),
                e.category,
                e.size,
                e.url.as_str(),
                e.via.as_ref().and_then(|v| v.reference.as_deref()),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                "A video - Best single file",
                Category::Video,
                Some(1234),
                "https://video.example/watch?v=1",
                Some("single")
            ),
            (
                "A video - Audio only",
                Category::Music,
                Some(1234),
                "https://video.example/watch?v=1",
                Some("audio")
            ),
        ]
    );
    assert!(info.entries.iter().all(|e| e
        .via
        .as_ref()
        .is_some_and(|v| v.plugin == "finder" && v.resolver == "media")));
}

#[tokio::test]
async fn addresses_nobody_wants_go_to_the_engine_as_before() {
    let rig = rig_with_resolvers().await;
    let app = axum::Router::new().route("/f.bin", axum::routing::get(|| async { vec![1u8; 100] }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let url = format!("http://{}/f.bin", listener.local_addr().expect("addr"));
    tokio::spawn(async move { axum::serve(listener, app).await });
    let info = rig.mgr.probe(&url, None).await.expect("probe");
    assert_eq!(info.kind, UrlKind::File);
    assert_eq!(info.filename, "f.bin");
}

#[tokio::test]
async fn sites_the_user_adds_in_a_setting_are_matched_too() {
    let rig = rig_with_resolvers().await;
    // Not claimed yet: the engine tries and cannot reach the made-up host.
    let before = rig
        .mgr
        .probe("https://other.example/clip", None)
        .await
        .expect_err("engine");
    assert!(matches!(before, Error::Engine(_)), "{before:?}");
    rig.mgr
        .plugin_save_settings(
            "finder".into(),
            values(&[(
                "extra",
                Some(text(
                    "https://nope.example/*\nhttps://other.example/*, https://third.example/*",
                )),
            )]),
        )
        .await
        .expect("save");
    for url in ["https://other.example/clip", "https://third.example/x"] {
        let info = rig.mgr.probe(url, None).await.expect("claimed");
        assert_eq!(info.entries.len(), 2, "{url}");
    }
}

#[tokio::test]
async fn a_plugin_that_is_off_or_a_login_keeps_the_engine_in_charge() {
    let rig = rig_with_resolvers().await;
    let creds = Some(fetchbox_core::Credentials {
        username: "u".into(),
        password: "p".into(),
    });
    let e = rig
        .mgr
        .probe("https://video.example/x", creds)
        .await
        .expect_err("engine");
    assert!(matches!(e, Error::Engine(_)), "{e:?}");
    rig.mgr
        .plugin_set_enabled("finder".into(), false)
        .await
        .expect("off");
    let e = rig
        .mgr
        .probe("https://video.example/x", None)
        .await
        .expect_err("engine");
    assert!(matches!(e, Error::Engine(_)), "{e:?}");
}

#[tokio::test]
async fn when_no_preset_can_run_the_answer_says_what_to_install() {
    let rig = rig_with_resolvers().await;
    // Make every preset need ffmpeg.
    let mut m = resolving();
    for c in m["resolvers"][1]["choices"]
        .as_array_mut()
        .expect("choices")
    {
        c["requires"] = json!(["ffmpeg"]);
    }
    let staged = rig
        .mgr
        .plugin_stage(package(&m, true))
        .await
        .expect("stage");
    rig.mgr.plugin_install(staged.token).await.expect("install");
    rig.wait_for("finder", PluginState::Ready).await;
    let e = rig
        .mgr
        .probe("https://video.example/x", None)
        .await
        .expect_err("nothing can run");
    assert!(invalid(e).contains("install ffmpeg first"));
}

#[tokio::test]
async fn a_program_that_prints_nonsense_is_a_clear_error() {
    let rig = Rig::new().await;
    let m = manifest(
        "garbled",
        json!({"resolvers": [{
            "id": "r", "match": ["https://x.example/*"],
            "probe": {"command": "bin/fake", "args": ["fail"]},
            "result": {"items": "/entries", "item": {"name": "/t", "url": "/u"}}
        }]}),
    );
    rig.install(&m, true).await;
    rig.mgr
        .plugin_set_enabled("garbled".into(), true)
        .await
        .expect("enable");
    rig.wait_for("garbled", PluginState::Ready).await;
    let e = rig
        .mgr
        .probe("https://x.example/a", None)
        .await
        .expect_err("fails");
    assert!(
        matches!(&e, Error::Engine(m) if m.starts_with("garbled:") && m.contains("boom")),
        "{e:?}"
    );
}

#[tokio::test]
async fn a_download_added_via_a_plugin_remembers_which_one_makes_it() {
    let rig = rig_with_resolvers().await;
    // A start time far ahead keeps it queued, so it stays a live download to collide with.
    let later = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
        + 3600;
    let mut first = add_request(
        "https://video.example/watch?v=1",
        Some(via("finder", "media", "audio")),
        Some("A video - Audio only"),
    );
    first.start_at = Some(later);
    let d = rig.mgr.add(first).await.expect("add");
    assert_eq!(d.plugin.as_deref(), Some("finder"));
    assert_eq!(
        d.filename, "A video - Audio only.m4a",
        "the preset's extension is added"
    );
    assert_eq!(d.category, Category::Music);
    assert_eq!(d.url, "https://video.example/watch?v=1");
    assert_eq!(d.source_kind, fetchbox_core::SourceKind::Http);

    // The same video again, the same way: a duplicate. As another preset: not.
    let again = rig
        .mgr
        .add(add_request(
            "https://video.example/watch?v=1",
            Some(via("finder", "media", "audio")),
            Some("A video - Audio only"),
        ))
        .await
        .expect_err("duplicate");
    assert!(matches!(again, Error::Conflict(_)), "{again:?}");
    let other = rig
        .mgr
        .add(add_request(
            "https://video.example/watch?v=1",
            Some(via("finder", "media", "single")),
            Some("A video - Best single file"),
        ))
        .await
        .expect("another preset is another download");
    assert_eq!(other.category, Category::Video);
    assert_ne!(other.id, d.id);
}

#[tokio::test]
async fn a_via_that_cannot_work_is_refused_before_anything_is_added() {
    let rig = rig_with_resolvers().await;
    let add = |v: Via, url: &str| {
        let rig = &rig;
        let url = url.to_owned();
        async move { rig.mgr.add(add_request(&url, Some(v), None)).await }
    };
    let e = add(via("nobody", "media", "audio"), "https://video.example/x")
        .await
        .expect_err("plugin");
    assert!(matches!(e, Error::InvalidInput(_)), "{e:?}");
    let e = add(via("finder", "nope", "audio"), "https://video.example/x")
        .await
        .expect_err("resolver");
    assert!(matches!(e, Error::InvalidInput(_)), "{e:?}");
    // `site` finds addresses; it does not make files.
    let e = add(via("finder", "site", "audio"), "https://video.example/x")
        .await
        .expect_err("no run");
    assert!(matches!(e, Error::InvalidInput(_)), "{e:?}");
    let e = add(via("finder", "media", "nope"), "https://video.example/x")
        .await
        .expect_err("preset");
    assert!(invalid(e).contains("no preset"));
    let e = add(via("finder", "media", "best"), "https://video.example/x")
        .await
        .expect_err("needs ffmpeg");
    assert!(invalid(e).contains("needs ffmpeg"));
    let e = add(via("finder", "media", "audio"), "ftp://video.example/x")
        .await
        .expect_err("scheme");
    assert!(matches!(e, Error::InvalidInput(_)), "{e:?}");
    let mut with_login = add_request(
        "https://video.example/x",
        Some(via("finder", "media", "audio")),
        None,
    );
    with_login.credentials = Some(fetchbox_core::Credentials {
        username: "u".into(),
        password: "p".into(),
    });
    assert!(matches!(
        rig.mgr.add(with_login).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(rig.mgr.list().await.expect("downloads").is_empty());

    // A plugin that is off cannot be used either.
    rig.mgr
        .plugin_set_enabled("finder".into(), false)
        .await
        .expect("off");
    let e = add(via("finder", "media", "audio"), "https://video.example/x")
        .await
        .expect_err("off");
    assert!(invalid(e).contains("is not running"));
}
