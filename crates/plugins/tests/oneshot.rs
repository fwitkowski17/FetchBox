//! Running commands once, against the stand-in plugin program.

mod common;

use std::time::{Duration, Instant};

use common::{env_in, fake_plugin};
use fetchbox_plugins::oneshot::{Runner, Spec};
use fetchbox_plugins::Error;

fn spec(root: &std::path::Path, args: &[&str], secs: u64) -> Spec {
    Spec::new(
        fake_plugin(),
        args.iter().map(|a| (*a).to_owned()).collect(),
        env_in(root),
        Duration::from_secs(secs),
    )
}

fn run_err(e: Error) -> String {
    assert!(matches!(e, Error::Run(_)), "{e:?}");
    e.to_string()
}

#[tokio::test]
async fn what_a_command_prints_is_returned() {
    let tmp = tempfile::tempdir().expect("tmp");
    let out = Runner::default()
        .run("demo", spec(tmp.path(), &["print-json"], 10))
        .await
        .expect("run");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(v["title"], "A video");
    assert_eq!(v["entries"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn a_failing_command_reports_its_exit_code_and_last_words() {
    let tmp = tempfile::tempdir().expect("tmp");
    let e = Runner::default()
        .run("demo", spec(tmp.path(), &["fail"], 10))
        .await
        .expect_err("fails");
    let msg = run_err(e);
    assert!(msg.contains("exited with code 2"), "{msg}");
    assert!(msg.ends_with("boom: something went wrong"), "{msg}");
}

#[tokio::test]
async fn input_is_passed_on_and_arguments_stay_whole() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec(tmp.path(), &["cat"], 10);
    s.stdin = Some(b"{\"event\":\"x\"}".to_vec());
    let out = Runner::default().run("demo", s).await.expect("run");
    assert_eq!(out.stdout, b"{\"event\":\"x\"}");

    // Characters a shell would act on are just characters.
    let nasty = "a b; rm -rf / $(x) `y` \"q\" 'r' *";
    let out = Runner::default()
        .run(
            "demo",
            spec(tmp.path(), &["args", nasty, "", "--flag=1"], 10),
        )
        .await
        .expect("run");
    let got: Vec<String> = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(got, [nasty, "", "--flag=1"]);
}

#[tokio::test]
async fn a_command_that_takes_too_long_is_killed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec(tmp.path(), &["sleep"], 1);
    s.timeout = Duration::from_millis(400);
    let started = Instant::now();
    let e = Runner::default().run("demo", s).await.expect_err("timeout");
    assert!(run_err(e).contains("took longer"), "timeout message");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn too_much_output_stops_the_command() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec(tmp.path(), &["flood"], 20);
    s.max_stdout = 4 * 1024 * 1024;
    let e = Runner::default().run("demo", s).await.expect_err("flood");
    assert!(
        run_err(e).contains("printed more than 4 MiB"),
        "overflow message"
    );
}

#[tokio::test]
async fn a_missing_program_is_explained() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec(tmp.path(), &[], 5);
    s.program = tmp.path().join("nope");
    let e = Runner::default().run("demo", s).await.expect_err("missing");
    assert!(run_err(e).contains("cannot run nope"), "message");
}

#[tokio::test]
async fn the_program_gets_a_clean_environment() {
    // A secret in FetchBox's own environment must not reach a plugin.
    std::env::set_var("FETCHBOX_PASSWORD", "hunter2");
    std::env::set_var("SOME_TOKEN", "abc");
    let tmp = tempfile::tempdir().expect("tmp");
    let mut env = env_in(tmp.path());
    env.extra.insert("MINE".into(), "yes".into());
    let mut s = spec(tmp.path(), &["env"], 10);
    s.env = env;
    let out = Runner::default().run("demo", s).await.expect("run");
    let vars: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&out.stdout).expect("json");
    assert!(!vars.contains_key("FETCHBOX_PASSWORD"));
    assert!(!vars.contains_key("SOME_TOKEN"));
    assert_eq!(
        vars.get("FETCHBOX_PLUGIN_ID").map(String::as_str),
        Some("demo")
    );
    assert_eq!(
        vars.get("FETCHBOX_API_VERSION").map(String::as_str),
        Some("1")
    );
    assert_eq!(vars.get("MINE").map(String::as_str), Some("yes"));
    let data = tmp.path().join("data");
    assert_eq!(
        vars.get("HOME"),
        Some(&data.join("home").display().to_string())
    );
    assert_eq!(
        vars.get("TMPDIR"),
        Some(&data.join("tmp").display().to_string())
    );
    assert_eq!(
        vars.get("FETCHBOX_PLUGIN_DATA"),
        Some(&data.display().to_string())
    );
}

#[tokio::test]
async fn one_plugin_runs_two_commands_at_a_time() {
    let tmp = tempfile::tempdir().expect("tmp");
    let runner = std::sync::Arc::new(Runner::default());
    let _ = fake_plugin();
    let started = Instant::now();
    // Each sleeps until its timeout; with two slots, four of them take two rounds.
    let tasks: Vec<_> = (0..4)
        .map(|_| {
            let runner = runner.clone();
            let mut s = spec(tmp.path(), &["sleep"], 1);
            s.timeout = Duration::from_millis(400);
            tokio::spawn(async move { runner.run("demo", s).await })
        })
        .collect();
    for t in tasks {
        t.await.expect("join").expect_err("timeout");
    }
    let one_plugin = started.elapsed();
    assert!(
        one_plugin >= Duration::from_millis(800),
        "four commands of one plugin must not all run at once: {one_plugin:?}"
    );
    // Another plugin is not held up by the first one's queue.
    let started = Instant::now();
    let tasks: Vec<_> = ["a", "b", "c", "d"]
        .into_iter()
        .map(|p| {
            let runner = runner.clone();
            let mut s = spec(tmp.path(), &["sleep"], 1);
            s.timeout = Duration::from_millis(400);
            tokio::spawn(async move { runner.run(p, s).await })
        })
        .collect();
    for t in tasks {
        t.await.expect("join").expect_err("timeout");
    }
    // One round instead of two: judged against the first run, so a busy machine does not matter.
    assert!(
        started.elapsed() < one_plugin,
        "four plugins are not held up by each other's queue: {:?} against {one_plugin:?}",
        started.elapsed()
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_timeout_kills_the_children_of_the_command_too() {
    let tmp = tempfile::tempdir().expect("tmp");
    let pidfile = tmp.path().join("child.pid");
    let mut s = spec(tmp.path(), &["child", pidfile.to_str().expect("utf8")], 1);
    s.timeout = Duration::from_millis(600);
    Runner::default().run("demo", s).await.expect_err("timeout");
    let pid: u32 = std::fs::read_to_string(&pidfile)
        .expect("pid")
        .trim()
        .parse()
        .expect("number");
    assert!(
        common::eventually(3, || !common::alive(pid)).await,
        "the child of the command is still running"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn what_a_finished_command_left_running_is_cleaned_up() {
    let tmp = tempfile::tempdir().expect("tmp");
    let out = Runner::default()
        .run("demo", spec(tmp.path(), &["exit-leaving-child"], 10))
        .await
        .expect("run");
    let pid: u32 = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("pid");
    assert!(
        common::eventually(3, || !common::alive(pid)).await,
        "left over"
    );
}
