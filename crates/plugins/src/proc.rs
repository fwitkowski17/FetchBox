//! Starting a plugin's programs: the environment they get, their own process group, and ending
//! them. A plugin's program is untrusted code run as the same user as FetchBox, so it starts with a
//! clean environment (none of FetchBox's variables, in particular no password) and is always
//! killed as a group, because a packaged program (PyInstaller, a shell wrapper) may start children.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::{Child, Command};

/// What a program is told about itself.
#[derive(Debug, Clone, Default)]
pub struct ChildEnv {
    pub plugin_id: String,
    pub plugin_dir: PathBuf,
    /// `plugin-data/<id>`: also the working directory.
    pub data_dir: PathBuf,
    /// Folders searched for programs (the dependencies' and the plugin's `bin`).
    pub path_dirs: Vec<PathBuf>,
    /// The manifest's `env` (already filled in).
    pub extra: BTreeMap<String, String>,
}

impl ChildEnv {
    /// The variables, and only these, the program starts with.
    pub fn vars(&self) -> Vec<(OsString, OsString)> {
        let mut v: Vec<(OsString, OsString)> = Vec::new();
        let mut put = |k: &str, val: OsString| v.push((k.into(), val));
        if let Ok(path) = std::env::join_paths(&self.path_dirs) {
            put("PATH", path);
        }
        let home = self.data_dir.join("home");
        let tmp = self.data_dir.join("tmp");
        put("HOME", home.into_os_string());
        for k in ["TMPDIR", "TMP", "TEMP"] {
            put(k, tmp.clone().into_os_string());
        }
        put("LANG", "C.UTF-8".into());
        put("FETCHBOX_PLUGIN_ID", self.plugin_id.clone().into());
        put(
            "FETCHBOX_PLUGIN_DIR",
            self.plugin_dir.clone().into_os_string(),
        );
        put(
            "FETCHBOX_PLUGIN_DATA",
            self.data_dir.clone().into_os_string(),
        );
        put(
            "FETCHBOX_API_VERSION",
            fetchbox_core::plugin::API_VERSION.to_string().into(),
        );
        if cfg!(windows) {
            // Windows programs fail to load without it.
            for k in ["SystemRoot", "SYSTEMROOT"] {
                if let Some(val) = std::env::var_os(k) {
                    put(k, val);
                }
            }
        }
        for (k, val) in &self.extra {
            put(k, val.clone().into());
        }
        v
    }
}

/// Make the folders a program is allowed to use.
pub fn prepare_dirs(data_dir: &Path) -> std::io::Result<()> {
    for d in ["home", "tmp", "state"] {
        std::fs::create_dir_all(data_dir.join(d))?;
    }
    Ok(())
}

/// A command ready to spawn: no shell, a clean environment, its own process group, all three
/// standard streams piped, killed if the handle is dropped.
pub fn command(program: &Path, args: &[String], env: &ChildEnv) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_clear()
        .envs(env.vars())
        .current_dir(&env.data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    cmd
}

/// Send a signal to the whole group of a child started with [`command`].
#[cfg(unix)]
fn signal_group(pid: u32, sig: rustix::process::Signal) {
    let pid = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw);
    if let Some(pid) = pid {
        // The group may be gone already; nothing to do about it then.
        let _ = rustix::process::kill_process_group(pid, sig);
    }
}

/// Ask a child and its descendants to stop (SIGTERM).
pub fn terminate(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        signal_group(pid, rustix::process::Signal::TERM);
        return;
    }
    let _ = child.start_kill();
}

/// Kill a child and its descendants.
pub fn kill(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        signal_group(pid, rustix::process::Signal::KILL);
    }
    let _ = child.start_kill();
}

/// Kill whatever is left of the group after the child itself has ended.
pub fn kill_leftovers(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        signal_group(pid, rustix::process::Signal::KILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// Cut a line of a program's output for a log or an error message.
pub fn clip(s: &str, max: usize) -> String {
    let clean: String = s
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect();
    if clean.chars().count() <= max {
        clean
    } else {
        let mut cut: String = clean.chars().take(max).collect();
        cut.push('…');
        cut
    }
}
