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

/// The stand-in plugin program (`crates/plugins/examples/fake_plugin.rs`), built if needed.
fn fake_plugin() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let exe = std::env::current_exe().expect("test exe");
        let profile = exe.parent().and_then(Path::parent).expect("target dir");
        let path = profile
            .join("examples")
            .join(format!("fake_plugin{}", std::env::consts::EXE_SUFFIX));
        if !path.exists() {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let mut cmd = std::process::Command::new(cargo);
            cmd.args([
                "build",
                "--example",
                "fake_plugin",
                "-p",
                "fetchbox-plugins",
            ]);
            if profile.ends_with("release") {
                cmd.arg("--release");
            }
            assert!(
                cmd.status().expect("cargo").success(),
                "building the example failed"
            );
        }
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
        let mgr = Self::boot(root.path(), mode, None).await;
        Rig { mgr, root }
    }

    /// A manager on the files of `root` (a second call is a restart of the server).
    async fn boot(root: &Path, mode: PluginsMode, github_api: Option<String>) -> Arc<Manager> {
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
        mgr: Rig::boot(rig.root.path(), PluginsMode::On, None).await,
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
    let mgr = Rig::boot(root.path(), PluginsMode::On, Some(api)).await;
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
