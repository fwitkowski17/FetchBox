//! A stand-in for a plugin's program, used by the tests of the plugin system (the plugins crate and
//! the server). It is a crate of its own so that building it is the same everywhere: `cargo test
//! --workspace` builds it, and a test run of one crate builds it by name. The first argument picks
//! what it does.
//!
//! One-shot modes: `run <out_dir> [key=value ...]` (makes a file like a downloader: see `run`),
//! `print-json` (a playlist), `print-single` (one video), `fail`, `sleep`, `flood`, `cat`, `env`, `args`, `child <pidfile>`
//! (starts a child, then sleeps), `exit-leaving-child` (prints the pid of a child it leaves running).
//!
//! Long-lived modes speak the JSON-lines protocol of `fetchbox_plugins::protocol`: `echo`,
//! `ignore-shutdown`, `no-init` (never answers `initialize`), `bad-init`, `crash-on-start`.
//! In those, requests are: `resolve` (answers with its params), `invoke` (`{"action":"fail"}` is an
//! error), `sleep` (`{"ms":N}` then answers), `silent` (never answers), `crash`, `big` (writes a line
//! of 2 MiB), `junk` (writes a line that is not JSON, then answers), `notify`, `env`, `spawn-child`,
//! `call-host` (sends a request of its own), `shutdown`. Notifications named `event` are appended
//! to `$FETCHBOX_PLUGIN_DATA/state/events.log`, and so are the answers it gets to `call-host`.

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

fn forever() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn say(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn state_file(name: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("FETCHBOX_PLUGIN_DATA")?).join("state");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(name))
}

fn append(name: &str, line: &str) {
    if let Some(p) = state_file(name) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn spawn_sleeper() -> u32 {
    let exe = std::env::current_exe().expect("exe");
    Command::new(exe)
        .arg("sleep")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("child")
        .id()
}

fn env_json() -> Value {
    Value::Object(
        std::env::vars()
            .map(|(k, v)| (k, Value::String(v)))
            .collect(),
    )
}

fn server(mode: &str) {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = v.get("id").cloned();
        let method = v["method"].as_str().unwrap_or("").to_owned();
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let reply = |result: Value| {
            if let Some(id) = &id {
                say(&json!({"id": id, "result": result}));
            }
        };
        if method.is_empty() {
            // An answer to a request this program made.
            if id.as_ref().is_some_and(|i| i == "p1") {
                append("events.log", &format!("answer {v}"));
            }
            continue;
        }
        match (mode, method.as_str()) {
            ("crash-on-start", _) => {
                eprintln!("fake plugin: cannot start");
                std::process::exit(1);
            }
            ("no-init", "initialize") => {}
            ("bad-init", "initialize") => {
                say(&json!({"id": id, "error": {"code": -1, "message": "no licence"}}));
            }
            (_, "initialize") => {
                reply(json!({"name": "fake"}));
                say(&json!({"method": "status", "params": {"text": "ready"}}));
                say(&json!({"method": "log", "params": {"level": "info", "message": "hello"}}));
                eprintln!("fake plugin: started");
            }
            (_, "resolve") => reply(params),
            (_, "invoke") => {
                if params["action"] == "fail" {
                    say(&json!({"id": id, "error": {"code": 7, "message": "it failed"}}));
                } else {
                    reply(json!({"echo": params}));
                }
            }
            (_, "sleep") => {
                std::thread::sleep(Duration::from_millis(params["ms"].as_u64().unwrap_or(100)));
                reply(json!({}));
            }
            (_, "silent") => {}
            (_, "crash") => std::process::exit(3),
            (_, "big") => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all("x".repeat(2 * 1024 * 1024).as_bytes());
                let _ = out.write_all(b"\n");
                let _ = out.flush();
            }
            (_, "junk") => {
                println!("this is not json");
                reply(json!({"after": "junk"}));
            }
            (_, "notify") => {
                say(
                    &json!({"method": "notify", "params": {"level": "warning", "message": "heads up"}}),
                );
                say(&json!({"method": "status", "params": {"text": ""}}));
                reply(json!({}));
            }
            (_, "env") => reply(env_json()),
            (_, "spawn-child") => {
                let pid = spawn_sleeper();
                append("child.pid", &pid.to_string());
                reply(json!({"pid": pid}));
            }
            (_, "call-host") => {
                say(&json!({"id": "p1", "method": "downloads.list", "params": {}}));
                reply(json!({"sent": true}));
            }
            (_, "event") => append("events.log", &format!("event {params}")),
            (_, "settings_changed") => append("events.log", &format!("settings {params}")),
            (m, "shutdown") => {
                reply(json!({}));
                if m != "ignore-shutdown" {
                    std::process::exit(0);
                }
            }
            _ => {
                if id.is_some() {
                    say(&json!({"id": id, "error": {"code": -32601, "message": "unknown method"}}));
                }
            }
        }
    }
    if mode == "ignore-shutdown" {
        // Standard input is closed; a program that ignores shutdown stays until it is signalled.
        forever();
    }
}

/// `run <out_dir> [key=value ...]`: writes `<name>.part` in `out_dir` chunk by chunk, printing
/// `@fetchbox progress <downloaded> <total> <speed>` after each, and renames it to `<name>` at the end.
/// It continues a `.part` that is already there, like a downloader does.
///
/// Keys: `name` (default `video.mp4`), `size` (4000), `chunks` (4), `delay` (milliseconds between
/// chunks, 30), `fail=<message>` (print it to standard error and exit with 3 after the first chunk),
/// `hang=1` (stop after the first chunk and wait for ever), `announce=1` (print
/// `@fetchbox file <path>` at the end) or `announce=<path>` (print that path instead), `na=1`
/// (progress numbers as `NA`).
fn run(args: &mut impl Iterator<Item = String>) {
    use std::collections::HashMap;
    let out = PathBuf::from(args.next().unwrap_or_default());
    let opts: HashMap<String, String> = args
        .filter_map(|a| a.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .collect();
    let get = |k: &str| opts.get(k).map(String::as_str);
    let num = |k: &str, d: u64| get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
    let name = get("name").unwrap_or("video.mp4").to_owned();
    let (size, chunks, delay) = (num("size", 4000), num("chunks", 4).max(1), num("delay", 30));
    let part = out.join(format!("{name}.part"));
    let mut have = std::fs::metadata(&part).map_or(0, |m| m.len());
    let mut first = true;
    while have < size {
        std::thread::sleep(Duration::from_millis(delay));
        let step = (size / chunks).max(1).min(size - have);
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part)
            .expect("part file");
        f.write_all(&vec![b'x'; usize::try_from(step).unwrap_or(1)])
            .expect("write");
        have += step;
        if get("na").is_some() {
            println!("@fetchbox progress NA NA NA");
        } else {
            println!("@fetchbox progress {have} {size} 1000.5");
        }
        let _ = std::io::stdout().flush();
        if first {
            first = false;
            if let Some(msg) = get("fail") {
                eprintln!("ERROR: {msg}");
                std::process::exit(3);
            }
            if get("hang").is_some() {
                forever();
            }
        }
    }
    let done = out.join(&name);
    std::fs::rename(&part, &done).expect("rename");
    match get("announce") {
        Some("1") => println!("@fetchbox file {}", done.display()),
        Some(other) => println!("@fetchbox file {other}"),
        None => {}
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    match mode.as_str() {
        "run" => run(&mut args),
        "print-json" => say(&json!({
            "title": "A video",
            "filesize": 1234,
            "entries": [
                {"title": "One", "url": "https://v.example/1"},
                {"title": "Two", "url": "https://v.example/2"}
            ]
        })),
        "print-single" => say(&json!({"title": "A video", "filesize": 1234})),
        "fail" => {
            eprintln!("starting up");
            eprintln!("boom: something went wrong");
            std::process::exit(2);
        }
        "sleep" => forever(),
        "flood" => {
            let chunk = vec![b'a'; 1024 * 1024];
            let mut out = std::io::stdout().lock();
            for _ in 0..40 {
                if out.write_all(&chunk).is_err() {
                    break;
                }
            }
        }
        "cat" => {
            let mut input = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut input);
            let _ = std::io::stdout().write_all(&input);
        }
        "env" => say(&env_json()),
        "args" => say(&json!(std::env::args().skip(2).collect::<Vec<_>>())),
        "child" => {
            let pid = spawn_sleeper();
            if let Some(file) = args.next() {
                let _ = std::fs::write(file, pid.to_string());
            }
            forever();
        }
        "exit-leaving-child" => {
            println!("{}", spawn_sleeper());
        }
        "echo" | "ignore-shutdown" | "no-init" | "bad-init" | "crash-on-start" => server(&mode),
        other => {
            eprintln!("fake_plugin: unknown mode {other:?}");
            std::process::exit(64);
        }
    }
}
