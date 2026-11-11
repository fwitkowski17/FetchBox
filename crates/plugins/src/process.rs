//! A plugin's long-lived program (SPEC 6.15): started with the plugin, talked to in JSON lines
//! (`protocol`), restarted when it dies, and stopped gracefully.
//!
//! One task per program supervises it. Everything the program can do wrong stays inside that task:
//! a line that is too long or not JSON, a request it never answers, a crash on start, a flood on
//! standard error. None of it can block the caller or FetchBox's own work: every call has a deadline
//! and the queue to the task is bounded.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fetchbox_core::plugin::{NoticeLevel, RestartPolicy};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot, watch};

use crate::proc::{self, ChildEnv};
use crate::protocol::{self, Incoming};
use crate::{Error, Result};

/// Lines of the program's standard error that are kept for the log view.
const LOG_KEEP: usize = 50;
/// Longest log line that is kept.
const LOG_LINE: usize = 500;
/// How many requests may wait for the supervisor.
const QUEUE: usize = 64;
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const TICK: Duration = Duration::from_millis(250);

/// The clock of the supervisor. The defaults are what runs; tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Time for the program to answer `initialize`.
    pub init_timeout: Duration,
    /// Time to leave the program after `shutdown`, and again after SIGTERM, before killing it.
    pub stop_grace: Duration,
    /// First wait before a restart; it doubles up to `backoff_max`.
    pub backoff_start: Duration,
    pub backoff_max: Duration,
    /// A program that ran this long counts as having started fine.
    pub stable_after: Duration,
    /// Failed starts within this window are counted ...
    pub failure_window: Duration,
    /// ... and this many of them give up until the user restarts the plugin.
    pub max_failures: usize,
    /// Requests in a row that got no answer before the program is restarted.
    pub max_timeouts: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            init_timeout: Duration::from_secs(10),
            stop_grace: Duration::from_secs(3),
            backoff_start: Duration::from_secs(1),
            backoff_max: Duration::from_secs(300),
            stable_after: Duration::from_secs(60),
            failure_window: Duration::from_secs(600),
            max_failures: 5,
            max_timeouts: 3,
        }
    }
}

/// How to start the program.
#[derive(Debug, Clone)]
pub struct Config {
    pub plugin_id: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: ChildEnv,
    pub restart: RestartPolicy,
    /// The parameters of `initialize`.
    pub initialize: Value,
    pub timing: Timing,
}

/// Where the program is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Starting,
    Running,
    /// Waiting to be started again.
    Backoff {
        secs: u64,
    },
    /// Gave up; `Process::restart` tries again.
    Failed(String),
    Stopped,
}

/// What happens that the owner wants to hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    State(State),
    /// The program's line about itself (`status`); `None` clears it.
    Status(Option<String>),
    /// A message for the user (`notify`).
    Notify {
        level: NoticeLevel,
        message: String,
    },
}

type Sink = Arc<dyn Fn(Event) + Send + Sync>;
type Logs = Arc<Mutex<VecDeque<String>>>;

enum Cmd {
    Request {
        method: String,
        params: Value,
        timeout: Duration,
        reply: oneshot::Sender<Result<Value>>,
    },
    Notify {
        method: String,
        params: Value,
    },
    Restart,
    Stop {
        ack: oneshot::Sender<()>,
    },
}

/// A handle to a supervised program. Dropping it stops the program.
#[derive(Debug)]
pub struct Process {
    cmd: mpsc::Sender<Cmd>,
    state: watch::Receiver<State>,
    logs: Logs,
    stop_grace: Duration,
}

impl Process {
    /// Start supervising. Needs a running tokio runtime. `on_event` is called from the supervisor
    /// task and must not block.
    pub fn spawn(cfg: Config, on_event: Arc<dyn Fn(Event) + Send + Sync>) -> Process {
        let (cmd, rx) = mpsc::channel(QUEUE);
        let (state_tx, state) = watch::channel(State::Starting);
        let logs: Logs = Arc::new(Mutex::new(VecDeque::new()));
        let stop_grace = cfg.timing.stop_grace;
        let sup = Supervisor {
            cfg,
            sink: on_event,
            state: state_tx,
            logs: logs.clone(),
        };
        tokio::spawn(sup.run(rx));
        Process {
            cmd,
            state,
            logs,
            stop_grace,
        }
    }

    pub fn state(&self) -> State {
        self.state.borrow().clone()
    }

    /// The last lines the program wrote to standard error, and its own log messages.
    pub fn logs(&self) -> Vec<String> {
        self.logs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    /// Ask the program something and wait for the answer, at most `timeout`.
    pub async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let (reply, answer) = oneshot::channel();
        self.cmd
            .send(Cmd::Request {
                method: method.to_owned(),
                params,
                timeout,
                reply,
            })
            .await
            .map_err(|_| Error::Run("the plugin's program is not supervised any more".into()))?;
        answer
            .await
            .map_err(|_| Error::Run("the plugin's program stopped".into()))?
    }

    /// Tell the program something without waiting. Returns false when the queue is full or the
    /// supervisor is gone (the message is dropped: delivery is best effort).
    pub fn notify(&self, method: &str, params: Value) -> bool {
        self.cmd
            .try_send(Cmd::Notify {
                method: method.to_owned(),
                params,
            })
            .is_ok()
    }

    /// Start it again now, also after it gave up.
    pub async fn restart(&self) {
        let _ = self.cmd.send(Cmd::Restart).await;
    }

    /// Stop it gracefully and wait until it has ended.
    pub async fn stop(&self) {
        let (ack, done) = oneshot::channel();
        if self.cmd.send(Cmd::Stop { ack }).await.is_ok() {
            // The supervisor needs up to two grace periods, plus a moment to reap.
            let _ = tokio::time::timeout(self.stop_grace * 2 + Duration::from_secs(2), done).await;
        }
    }
}

enum Line {
    Msg(String),
    Oversize,
    Eof,
}

/// Read lines of at most `protocol::MAX_LINE` bytes.
async fn read_lines(out: ChildStdout, tx: mpsc::Sender<Line>) {
    let mut reader = BufReader::new(out);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let read = (&mut reader)
            .take(protocol::MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut buf)
            .await;
        let sent = match read {
            Ok(0) | Err(_) => {
                let _ = tx.send(Line::Eof).await;
                return;
            }
            Ok(_) if !buf.ends_with(b"\n") && buf.len() > protocol::MAX_LINE => {
                let _ = tx.send(Line::Oversize).await;
                return;
            }
            Ok(_) => {
                let text = String::from_utf8_lossy(&buf);
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                tx.send(Line::Msg(text.to_owned())).await
            }
        };
        if sent.is_err() {
            return;
        }
    }
}

fn keep(logs: &Logs, line: String) {
    let mut q = logs.lock().unwrap_or_else(|e| e.into_inner());
    if q.len() >= LOG_KEEP {
        q.pop_front();
    }
    q.push_back(line);
}

/// Keep what the program writes to standard error.
async fn read_stderr(err: ChildStderr, logs: Logs, plugin: String) {
    let mut reader = BufReader::new(err);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match (&mut reader).take(4096).read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let line = proc::clip(String::from_utf8_lossy(&buf).trim(), LOG_LINE);
                if !line.is_empty() {
                    tracing::debug!(target: "plugin", plugin = %plugin, "{line}");
                    keep(&logs, line);
                }
            }
        }
    }
}

fn level_of(s: &str) -> NoticeLevel {
    match s {
        "success" => NoticeLevel::Success,
        "warning" | "warn" => NoticeLevel::Warning,
        "error" => NoticeLevel::Error,
        _ => NoticeLevel::Info,
    }
}

struct Pending {
    reply: Option<oneshot::Sender<Result<Value>>>,
    method: String,
    deadline: Instant,
    init: bool,
}

enum SessionEnd {
    /// The owner asked for a stop.
    Stopped(Option<oneshot::Sender<()>>),
    /// The owner asked for a restart.
    Restart,
    /// The program ended or was ended by the supervisor.
    Exited {
        ok: bool,
        ran: Duration,
        why: String,
    },
}

struct Supervisor {
    cfg: Config,
    sink: Sink,
    state: watch::Sender<State>,
    logs: Logs,
}

impl Supervisor {
    fn set_state(&self, s: State) {
        let _ = self.state.send(s.clone());
        (self.sink)(Event::State(s));
    }

    /// The program's last words: the last line it wrote to standard error.
    fn last_words(&self) -> String {
        self.logs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .back()
            .cloned()
            .unwrap_or_default()
    }

    /// Wait for the owner to decide, answering requests with "not running". Returns the command
    /// that ends the wait, or `None` when the handle is gone.
    async fn idle(&self, rx: &mut mpsc::Receiver<Cmd>) -> Option<Cmd> {
        loop {
            match rx.recv().await? {
                Cmd::Request { reply, .. } => {
                    let _ = reply.send(Err(Error::Run(format!(
                        "the plugin's program is not running ({})",
                        describe(&self.state.borrow())
                    ))));
                }
                Cmd::Notify { .. } => {}
                c @ (Cmd::Restart | Cmd::Stop { .. }) => return Some(c),
            }
        }
    }

    async fn run(self, mut rx: mpsc::Receiver<Cmd>) {
        let t = self.cfg.timing;
        let mut failures: VecDeque<Instant> = VecDeque::new();
        let mut backoff = t.backoff_start;
        loop {
            self.set_state(State::Starting);
            let end = self.session(&mut rx).await;
            let (ok, ran, why) = match end {
                SessionEnd::Stopped(ack) => {
                    self.set_state(State::Stopped);
                    if let Some(a) = ack {
                        let _ = a.send(());
                    }
                    return;
                }
                SessionEnd::Restart => {
                    failures.clear();
                    backoff = t.backoff_start;
                    continue;
                }
                SessionEnd::Exited { ok, ran, why } => (ok, ran, why),
            };
            tracing::info!(
                target: "activity", plugin = %self.cfg.plugin_id, reason = %why,
                "plugin program ended"
            );
            let again = match self.cfg.restart {
                RestartPolicy::Always => true,
                RestartPolicy::OnFailure => !ok,
                RestartPolicy::Never => false,
            };
            let now = Instant::now();
            if ran >= t.stable_after {
                failures.clear();
                backoff = t.backoff_start;
            } else {
                failures.push_back(now);
                while failures
                    .front()
                    .is_some_and(|f| now.duration_since(*f) > t.failure_window)
                {
                    failures.pop_front();
                }
            }
            let gave_up = failures.len() >= t.max_failures;
            if !again || gave_up {
                if ok && !gave_up {
                    self.set_state(State::Stopped);
                } else {
                    let msg = if gave_up {
                        format!("it keeps failing: {why}")
                    } else {
                        why
                    };
                    self.set_state(State::Failed(msg));
                }
                match self.idle(&mut rx).await {
                    Some(Cmd::Restart) => {
                        failures.clear();
                        backoff = t.backoff_start;
                        continue;
                    }
                    Some(Cmd::Stop { ack }) => {
                        self.set_state(State::Stopped);
                        let _ = ack.send(());
                        return;
                    }
                    _ => return,
                }
            }
            // Wait, then start again.
            self.set_state(State::Backoff {
                secs: backoff.as_secs().max(1),
            });
            let sleep = tokio::time::sleep(backoff);
            tokio::pin!(sleep);
            loop {
                tokio::select! {
                    () = &mut sleep => break,
                    cmd = rx.recv() => match cmd {
                        None => return,
                        Some(Cmd::Request { reply, .. }) => {
                            let _ = reply.send(Err(Error::Run("the plugin's program is restarting".into())));
                        }
                        Some(Cmd::Notify { .. }) => {}
                        Some(Cmd::Restart) => { failures.clear(); backoff = t.backoff_start; break; }
                        Some(Cmd::Stop { ack }) => {
                            self.set_state(State::Stopped);
                            let _ = ack.send(());
                            return;
                        }
                    }
                }
            }
            backoff = (backoff * 2).min(t.backoff_max);
        }
    }

    fn notification(&self, method: &str, params: &Value) {
        let text = |k: &str| {
            params
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        match method {
            "log" => {
                let (level, msg) = (text("level"), proc::clip(&text("message"), LOG_LINE));
                let plugin = &self.cfg.plugin_id;
                match level_of(&level) {
                    NoticeLevel::Error => {
                        tracing::warn!(target: "plugin", plugin = %plugin, "{msg}")
                    }
                    NoticeLevel::Warning => {
                        tracing::warn!(target: "plugin", plugin = %plugin, "{msg}")
                    }
                    _ => tracing::debug!(target: "plugin", plugin = %plugin, "{msg}"),
                }
                keep(
                    &self.logs,
                    format!("[{}] {msg}", if level.is_empty() { "info" } else { &level }),
                );
            }
            "status" => {
                let t = text("text");
                (self.sink)(Event::Status(
                    (!t.trim().is_empty()).then(|| proc::clip(&t, 200)),
                ));
            }
            "notify" => {
                let message = proc::clip(&text("message"), 500);
                if !message.is_empty() {
                    (self.sink)(Event::Notify {
                        level: level_of(&text("level")),
                        message,
                    });
                }
            }
            _ => {
                tracing::debug!(target: "plugin", plugin = %self.cfg.plugin_id, method, "unknown notification")
            }
        }
    }

    /// One run of the program, from start to end.
    async fn session(&self, rx: &mut mpsc::Receiver<Cmd>) -> SessionEnd {
        let t = self.cfg.timing;
        let started = Instant::now();
        let fail = |why: String| SessionEnd::Exited {
            ok: false,
            ran: started.elapsed(),
            why,
        };
        if let Err(e) = proc::prepare_dirs(&self.cfg.env.data_dir) {
            return fail(format!("cannot prepare its folders: {e}"));
        }
        let mut child =
            match proc::command(&self.cfg.program, &self.cfg.args, &self.cfg.env).spawn() {
                Ok(c) => c,
                Err(e) => return fail(format!("cannot start: {e}")),
            };
        let pid = child.id();
        let (Some(mut stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            proc::kill(&mut child);
            return fail("cannot talk to it".into());
        };
        let (ltx, mut lines) = mpsc::channel::<Line>(64);
        let readers = [
            tokio::spawn(read_lines(stdout, ltx)),
            tokio::spawn(read_stderr(
                stderr,
                self.logs.clone(),
                self.cfg.plugin_id.clone(),
            )),
        ];

        let mut next_id = 1u64;
        let mut pending: HashMap<u64, Pending> = HashMap::new();
        let init_id = next_id;
        next_id += 1;
        pending.insert(
            init_id,
            Pending {
                reply: None,
                method: "initialize".into(),
                deadline: started + t.init_timeout,
                init: true,
            },
        );
        let mut initialized = false;
        let mut timeouts = 0u32;
        let init_line = protocol::request_line(init_id, "initialize", &self.cfg.initialize);
        if !write_line(&mut stdin, &init_line).await {
            proc::kill(&mut child);
            return fail("cannot send to it".into());
        }
        let mut tick = tokio::time::interval(TICK);

        let end = loop {
            tokio::select! {
                line = lines.recv() => match line {
                    None | Some(Line::Eof) => {
                        // Standard output closed: the program is going away (or has closed it on purpose).
                        match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                            Ok(Ok(s)) => break self.exit_end(s, started),
                            _ => { proc::kill(&mut child); break fail("closed its output".into()); }
                        }
                    }
                    Some(Line::Oversize) => {
                        proc::kill(&mut child);
                        break fail(format!("wrote a line longer than {} MiB", protocol::MAX_LINE / (1024 * 1024)));
                    }
                    Some(Line::Msg(text)) => match protocol::parse_line(&text) {
                        Ok(Incoming::Response { id, result }) => {
                            timeouts = 0;
                            let Some(p) = pending.remove(&id) else { continue };
                            if p.init {
                                match result {
                                    Ok(_) => {
                                        initialized = true;
                                        self.set_state(State::Running);
                                        tracing::info!(target: "activity", plugin = %self.cfg.plugin_id, "plugin program started");
                                    }
                                    Err(e) => {
                                        proc::kill(&mut child);
                                        break fail(format!("refused to start: {}", e.message));
                                    }
                                }
                            } else if let Some(reply) = p.reply {
                                let _ = reply.send(result.map_err(|e| Error::Run(format!("{}: {}", p.method, e.message))));
                            }
                        }
                        Ok(Incoming::Notification { method, params }) => self.notification(&method, &params),
                        Ok(Incoming::Request { id, .. }) => {
                            let line = protocol::error_response_line(&id, -32601, "not implemented");
                            let _ = write_line(&mut stdin, &line).await;
                        }
                        Err(why) => tracing::debug!(target: "plugin", plugin = %self.cfg.plugin_id, "ignored a line: {why}"),
                    },
                },
                cmd = rx.recv() => match cmd {
                    None => {
                        graceful_stop(&mut child, &mut stdin, t.stop_grace, &mut next_id).await;
                        break SessionEnd::Stopped(None);
                    }
                    Some(Cmd::Stop { ack }) => {
                        graceful_stop(&mut child, &mut stdin, t.stop_grace, &mut next_id).await;
                        break SessionEnd::Stopped(Some(ack));
                    }
                    Some(Cmd::Restart) => {
                        graceful_stop(&mut child, &mut stdin, t.stop_grace, &mut next_id).await;
                        break SessionEnd::Restart;
                    }
                    Some(Cmd::Request { method, params, timeout, reply }) => {
                        if !initialized {
                            let _ = reply.send(Err(Error::Run("the plugin's program is still starting".into())));
                            continue;
                        }
                        let id = next_id;
                        next_id += 1;
                        let line = protocol::request_line(id, &method, &params);
                        if line.len() > protocol::MAX_LINE {
                            let _ = reply.send(Err(Error::Run(format!("{method}: the request is too large"))));
                            continue;
                        }
                        pending.insert(id, Pending { reply: Some(reply), method, deadline: Instant::now() + timeout, init: false });
                        if !write_line(&mut stdin, &line).await {
                            proc::kill(&mut child);
                            break fail("stopped reading its input".into());
                        }
                    }
                    Some(Cmd::Notify { method, params }) => {
                        if initialized {
                            let line = protocol::notification_line(&method, &params);
                            if line.len() <= protocol::MAX_LINE && !write_line(&mut stdin, &line).await {
                                proc::kill(&mut child);
                                break fail("stopped reading its input".into());
                            }
                        }
                    }
                },
                _ = tick.tick() => {
                    let now = Instant::now();
                    let expired: Vec<u64> = pending.iter().filter(|(_, p)| p.deadline <= now).map(|(id, _)| *id).collect();
                    let mut verdict = None;
                    for id in expired {
                        let Some(p) = pending.remove(&id) else { continue };
                        if p.init {
                            verdict = Some(format!("did not answer `initialize` within {} s", t.init_timeout.as_secs_f32().ceil()));
                        } else {
                            timeouts += 1;
                            if let Some(reply) = p.reply {
                                let _ = reply.send(Err(Error::Run(format!("{} timed out: the plugin did not answer", p.method))));
                            }
                            if timeouts >= t.max_timeouts && verdict.is_none() {
                                verdict = Some("stopped answering".into());
                            }
                        }
                    }
                    if let Some(why) = verdict {
                        proc::kill(&mut child);
                        break fail(why);
                    }
                },
                status = child.wait() => match status {
                    Ok(s) => break self.exit_end(s, started),
                    Err(e) => break fail(format!("cannot wait for it: {e}")),
                },
            }
        };

        // Whatever happened, nothing waits for an answer any more, and nothing is left running.
        for (_, p) in pending.drain() {
            if let Some(reply) = p.reply {
                let _ = reply.send(Err(Error::Run("the plugin's program stopped".into())));
            }
        }
        proc::kill_leftovers(pid);
        let _ = child.wait().await;
        for r in readers {
            r.abort();
        }
        end
    }

    fn exit_end(&self, s: std::process::ExitStatus, started: Instant) -> SessionEnd {
        let last = self.last_words();
        let how = s.code().map_or_else(
            || "was killed".to_owned(),
            |c| format!("exited with code {c}"),
        );
        SessionEnd::Exited {
            ok: s.success(),
            ran: started.elapsed(),
            why: if last.is_empty() {
                how
            } else {
                format!("{how}: {last}")
            },
        }
    }
}

fn describe(s: &State) -> String {
    match s {
        State::Starting => "starting".into(),
        State::Running => "running".into(),
        State::Backoff { .. } => "restarting soon".into(),
        State::Failed(why) => format!("failed: {why}"),
        State::Stopped => "stopped".into(),
    }
}

/// Write one line to the program, giving up if it does not read.
async fn write_line(stdin: &mut ChildStdin, line: &str) -> bool {
    matches!(
        tokio::time::timeout(WRITE_TIMEOUT, async {
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await
        })
        .await,
        Ok(Ok(()))
    )
}

/// `shutdown`, then SIGTERM, then SIGKILL, each after a grace period.
async fn graceful_stop(
    child: &mut tokio::process::Child,
    stdin: &mut ChildStdin,
    grace: Duration,
    next_id: &mut u64,
) {
    let id = *next_id;
    *next_id += 1;
    let _ = write_line(stdin, &protocol::request_line(id, "shutdown", &json!({}))).await;
    let _ = stdin.shutdown().await;
    if tokio::time::timeout(grace, child.wait()).await.is_ok() {
        return;
    }
    proc::terminate(child);
    if tokio::time::timeout(grace, child.wait()).await.is_ok() {
        return;
    }
    proc::kill(child);
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
}
