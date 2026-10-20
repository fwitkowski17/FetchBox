//! Shared by the integration tests: where the stand-in plugin program is, and a clean environment
//! for it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fetchbox_plugins::proc::ChildEnv;

/// The stand-in plugin program (`crates/test_plugin`). `cargo test --workspace` builds it, a run of
/// one crate does not, so it is built here by name (a no-op when it is current).
pub fn fake_plugin() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let exe = std::env::current_exe().expect("test exe");
        let profile_dir = exe.parent().and_then(Path::parent).expect("target dir");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut cmd = std::process::Command::new(cargo);
        cmd.args(["build", "--quiet", "-p", "fetchbox-test-plugin"]);
        if profile_dir.ends_with("release") {
            cmd.arg("--release");
        }
        assert!(
            cmd.status().expect("cargo").success(),
            "building the stand-in plugin failed"
        );
        let path = profile_dir.join(format!("fake-plugin{}", std::env::consts::EXE_SUFFIX));
        assert!(path.exists(), "no {}", path.display());
        path
    })
    .clone()
}

/// An environment for a plugin called `demo` whose data lives in `root/data`.
pub fn env_in(root: &Path) -> ChildEnv {
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    ChildEnv {
        plugin_id: "demo".into(),
        plugin_dir: root.join("plugin"),
        data_dir,
        path_dirs: vec![root.join("plugin").join("bin")],
        extra: Default::default(),
    }
}

/// Is a process with this id still there? (Linux only: other systems cannot tell cheaply.)
#[cfg(target_os = "linux")]
pub fn alive(pid: u32) -> bool {
    // A zombie counts as gone: it has been killed, only its parent has not collected it yet.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| {
            !s.rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z')
        })
        .unwrap_or(false)
}

/// Wait until `cond` holds, for at most `secs` seconds.
pub async fn eventually(secs: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if cond() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    cond()
}
