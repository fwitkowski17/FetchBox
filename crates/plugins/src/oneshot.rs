//! Running a command once and collecting what it prints (a probe, an action, a hook that runs a
//! program). The command gets a clean environment and its own process group; output and time are
//! limited, and when a limit is hit the whole group is killed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

use crate::proc::{self, ChildEnv};
use crate::{Error, Result};

/// Most programs running at once, over all plugins.
const GLOBAL_LIMIT: usize = 4;
/// Most programs of one plugin running at once.
const PLUGIN_LIMIT: usize = 2;
/// How much of what a program printed on stderr is kept (the end of it).
const STDERR_KEEP: usize = 64 * 1024;
/// Most standard output that is read.
pub const MAX_STDOUT: usize = 8 * 1024 * 1024;
/// Time to let a killed program be reaped.
const REAP: Duration = Duration::from_secs(2);

/// A command to run once.
#[derive(Debug, Clone)]
pub struct Spec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: ChildEnv,
    /// Written to its standard input, which is then closed.
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    pub max_stdout: usize,
}

impl Spec {
    pub fn new(program: PathBuf, args: Vec<String>, env: ChildEnv, timeout: Duration) -> Self {
        Spec {
            program,
            args,
            env,
            stdin: None,
            timeout,
            max_stdout: MAX_STDOUT,
        }
    }
}

/// What a command printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub stdout: Vec<u8>,
    /// The end of standard error.
    pub stderr: String,
}

/// Runs commands, a few at a time.
#[derive(Debug)]
pub struct Runner {
    global: Arc<Semaphore>,
    per_plugin: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl Default for Runner {
    fn default() -> Self {
        Runner {
            global: Arc::new(Semaphore::new(GLOBAL_LIMIT)),
            per_plugin: Mutex::new(HashMap::new()),
        }
    }
}

/// The last non-empty line of a program's standard error, for an error message.
pub fn last_line(stderr: &str) -> String {
    stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map_or_else(String::new, |l| proc::clip(l, 300))
}

fn name_of(program: &std::path::Path) -> String {
    program.file_name().map_or_else(
        || "the program".into(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Read up to `max` bytes; the flag says there was more.
async fn read_capped(
    mut r: impl AsyncReadExt + Unpin,
    max: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 16 * 1024];
    loop {
        let n = r.read(&mut chunk).await?;
        if n == 0 {
            return Ok((buf, false));
        }
        if buf.len() + n > max {
            buf.extend_from_slice(&chunk[..max - buf.len()]);
            return Ok((buf, true));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Read to the end, keeping only the last `keep` bytes.
async fn read_tail(mut r: impl AsyncReadExt + Unpin, keep: usize) -> String {
    let mut tail: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 8 * 1024];
    while let Ok(n) = r.read(&mut chunk).await {
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..n]);
        if tail.len() > keep {
            tail.drain(..tail.len() - keep);
        }
    }
    String::from_utf8_lossy(&tail).into_owned()
}

impl Runner {
    fn plugin_gate(&self, plugin: &str) -> Arc<Semaphore> {
        let mut map = self.per_plugin.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(plugin.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(PLUGIN_LIMIT)))
            .clone()
    }

    /// Run a command and return what it printed. A non-zero exit, a timeout and too much output are
    /// errors; the message ends with the last line the program wrote to standard error.
    pub async fn run(&self, plugin: &str, spec: Spec) -> Result<Output> {
        let gate = self.plugin_gate(plugin);
        let _plugin_turn = gate
            .acquire_owned()
            .await
            .map_err(|_| Error::Run("the plugin runner is shut down".into()))?;
        let _global_turn = self
            .global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Run("the plugin runner is shut down".into()))?;

        let name = name_of(&spec.program);
        let mut child = proc::command(&spec.program, &spec.args, &spec.env)
            .spawn()
            .map_err(|e| Error::Run(format!("cannot run {name}: {e}")))?;
        let pid = child.id();
        let (Some(mut stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            proc::kill(&mut child);
            return Err(Error::Run(format!("cannot talk to {name}")));
        };
        let payload = spec.stdin.clone();
        // A program that never reads its input must not block us: the write is its own task.
        tokio::spawn(async move {
            if let Some(p) = payload {
                let _ = stdin.write_all(&p).await;
            }
            let _ = stdin.shutdown().await;
        });
        let max = spec.max_stdout;
        let out_task = tokio::spawn(read_capped(stdout, max));
        let err_task = tokio::spawn(read_tail(stderr, STDERR_KEEP));

        let finished = tokio::time::timeout(spec.timeout, async {
            let (out, overflow) = out_task
                .await
                .map_err(|e| Error::Run(e.to_string()))?
                .map_err(Error::Io)?;
            if overflow {
                return Ok::<_, Error>((out, true, None));
            }
            let status = child.wait().await.map_err(Error::Io)?;
            Ok((out, false, Some(status)))
        })
        .await;

        let (stdout, overflow, status) = match finished {
            Err(_) => {
                proc::kill(&mut child);
                let _ = tokio::time::timeout(REAP, child.wait()).await;
                return Err(Error::Run(format!(
                    "{name} took longer than {} s and was stopped",
                    spec.timeout.as_secs()
                )));
            }
            Ok(Err(e)) => {
                proc::kill(&mut child);
                return Err(e);
            }
            Ok(Ok(done)) => done,
        };
        if overflow {
            proc::kill(&mut child);
            let _ = tokio::time::timeout(REAP, child.wait()).await;
            return Err(Error::Run(format!(
                "{name} printed more than {} MiB and was stopped",
                max / (1024 * 1024)
            )));
        }
        // Anything the program left running in its group goes with it.
        proc::kill_leftovers(pid);
        let stderr = tokio::time::timeout(REAP, err_task)
            .await
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or_default();
        match status {
            Some(s) if s.success() => Ok(Output { stdout, stderr }),
            Some(s) => {
                let why = last_line(&stderr);
                let code = s.code().map_or_else(
                    || "was killed".to_owned(),
                    |c| format!("exited with code {c}"),
                );
                Err(Error::Run(if why.is_empty() {
                    format!("{name} {code}")
                } else {
                    format!("{name} {code}: {why}")
                }))
            }
            None => Err(Error::Run(format!("{name} did not finish"))),
        }
    }
}
