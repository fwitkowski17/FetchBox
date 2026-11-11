//! The supervised long-lived program, against the stand-in plugin program.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{env_in, eventually, fake_plugin};
use fetchbox_core::plugin::{NoticeLevel, RestartPolicy};
use fetchbox_plugins::process::{Config, Event, Process, State, Timing};
use serde_json::{json, Value};

type Events = Arc<Mutex<Vec<Event>>>;

fn timing() -> Timing {
    Timing {
        init_timeout: Duration::from_secs(2),
        stop_grace: Duration::from_millis(500),
        backoff_start: Duration::from_millis(50),
        backoff_max: Duration::from_millis(200),
        stable_after: Duration::from_secs(60),
        failure_window: Duration::from_secs(600),
        max_failures: 3,
        max_timeouts: 2,
    }
}

fn start(root: &std::path::Path, mode: &str, edit: impl FnOnce(&mut Config)) -> (Process, Events) {
    let mut cfg = Config {
        plugin_id: "demo".into(),
        program: fake_plugin(),
        args: vec![mode.into()],
        env: env_in(root),
        restart: RestartPolicy::OnFailure,
        initialize: json!({"locale": "en"}),
        timing: timing(),
    };
    edit(&mut cfg);
    let events: Events = Arc::default();
    let sink = events.clone();
    let p = Process::spawn(cfg, Arc::new(move |e| sink.lock().expect("lock").push(e)));
    (p, events)
}

const SEC: Duration = Duration::from_secs(2);

async fn running(p: &Process) {
    assert!(
        eventually(10, || p.state() == State::Running).await,
        "never ran: {:?}",
        p.state()
    );
}

fn saw(events: &Events, f: impl Fn(&Event) -> bool) -> bool {
    events.lock().expect("lock").iter().any(f)
}

fn log_file(root: &std::path::Path) -> String {
    std::fs::read_to_string(root.join("data/state/events.log")).unwrap_or_default()
}

#[tokio::test]
async fn it_starts_answers_and_stops() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, events) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    assert!(saw(&events, |e| *e == Event::Status(Some("ready".into()))));

    assert_eq!(
        p.request("resolve", json!({"a": 1}), SEC)
            .await
            .expect("answer"),
        json!({"a": 1})
    );
    let e = p
        .request("invoke", json!({"action": "fail"}), SEC)
        .await
        .expect_err("error");
    assert!(e.to_string().contains("invoke: it failed"), "{e}");
    let e = p
        .request("nope", json!({}), SEC)
        .await
        .expect_err("unknown");
    assert!(e.to_string().contains("unknown method"), "{e}");

    let logs = p.logs();
    assert!(logs.iter().any(|l| l == "[info] hello"), "{logs:?}");
    assert!(eventually(3, || p.logs().iter().any(|l| l == "fake plugin: started")).await);

    p.stop().await;
    assert_eq!(p.state(), State::Stopped);
    let e = p
        .request("resolve", json!({}), SEC)
        .await
        .expect_err("stopped");
    assert!(
        e.to_string().contains("not running") || e.to_string().contains("not supervised"),
        "{e}"
    );
}

#[tokio::test]
async fn a_request_that_is_never_answered_times_out_and_the_program_keeps_working() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _events) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    let e = p
        .request("silent", json!({}), Duration::from_millis(300))
        .await
        .expect_err("timeout");
    assert!(e.to_string().contains("timed out"), "{e}");
    // One miss is not a reason to restart; an answer clears the count.
    assert_eq!(
        p.request("resolve", json!(1), SEC).await.expect("answer"),
        json!(1)
    );
    p.request("silent", json!({}), Duration::from_millis(300))
        .await
        .expect_err("timeout");
    assert_eq!(
        p.request("resolve", json!(2), SEC).await.expect("answer"),
        json!(2)
    );
    assert_eq!(p.state(), State::Running);
    p.stop().await;
}

#[tokio::test]
async fn a_program_that_stops_answering_is_restarted() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, events) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    for _ in 0..2 {
        p.request("silent", json!({}), Duration::from_millis(300))
            .await
            .expect_err("timeout");
    }
    assert!(
        eventually(10, || saw(&events, |e| matches!(
            e,
            Event::State(State::Backoff { .. })
        )))
        .await,
        "no restart"
    );
    running(&p).await;
    assert_eq!(
        p.request("resolve", json!(3), SEC).await.expect("answer"),
        json!(3)
    );
    p.stop().await;
}

#[tokio::test]
async fn a_crash_is_survived() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, events) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    let e = p.request("crash", json!({}), SEC).await.expect_err("crash");
    assert!(e.to_string().contains("stopped"), "{e}");
    assert!(
        eventually(10, || saw(&events, |e| matches!(
            e,
            Event::State(State::Backoff { .. })
        )))
        .await,
        "no restart planned"
    );
    running(&p).await;
    assert_eq!(
        p.request("resolve", json!(4), SEC).await.expect("answer"),
        json!(4)
    );
    p.stop().await;
}

#[tokio::test]
async fn a_program_that_cannot_start_is_given_up_on_until_the_user_restarts_it() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, events) = start(tmp.path(), "crash-on-start", |_| {});
    let failed = eventually(15, || matches!(p.state(), State::Failed(_))).await;
    assert!(failed, "{:?}", p.state());
    let State::Failed(why) = p.state() else {
        unreachable!()
    };
    assert!(
        why.contains("keeps failing") && why.contains("exited with code 1"),
        "{why}"
    );
    let e = p
        .request("resolve", json!({}), SEC)
        .await
        .expect_err("down");
    assert!(e.to_string().contains("not running"), "{e}");

    let starts = || {
        events
            .lock()
            .expect("lock")
            .iter()
            .filter(|e| **e == Event::State(State::Starting))
            .count()
    };
    let before = starts();
    p.restart().await;
    assert!(
        eventually(10, || starts() > before).await,
        "restart did nothing"
    );
    assert!(eventually(15, || matches!(p.state(), State::Failed(_))).await);
    p.stop().await;
    assert_eq!(p.state(), State::Stopped);
}

#[tokio::test]
async fn a_program_that_never_initialises_is_killed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _) = start(tmp.path(), "no-init", |c| {
        c.timing.init_timeout = Duration::from_millis(300)
    });
    assert!(
        eventually(15, || matches!(p.state(), State::Failed(_))).await,
        "{:?}",
        p.state()
    );
    let State::Failed(why) = p.state() else {
        unreachable!()
    };
    assert!(why.contains("did not answer `initialize`"), "{why}");
}

#[tokio::test]
async fn a_program_that_refuses_to_initialise_says_why() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _) = start(tmp.path(), "bad-init", |_| {});
    assert!(
        eventually(15, || matches!(p.state(), State::Failed(_))).await,
        "{:?}",
        p.state()
    );
    let State::Failed(why) = p.state() else {
        unreachable!()
    };
    assert!(why.contains("refused to start: no licence"), "{why}");
}

#[tokio::test]
async fn a_line_that_is_too_long_restarts_the_program() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, events) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    let e = p
        .request("big", json!({}), SEC)
        .await
        .expect_err("too long");
    assert!(e.to_string().contains("stopped"), "{e}");
    assert!(
        eventually(10, || saw(&events, |e| matches!(
            e,
            Event::State(State::Backoff { .. })
        )))
        .await
    );
    running(&p).await;
    p.stop().await;
}

#[tokio::test]
async fn lines_that_are_not_json_are_ignored() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    assert_eq!(
        p.request("junk", json!({}), SEC).await.expect("answer"),
        json!({"after": "junk"})
    );
    p.stop().await;
}

#[tokio::test]
async fn notifications_reach_the_owner_and_events_reach_the_program() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, events) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    p.request("notify", json!({}), SEC).await.expect("answer");
    assert!(saw(&events, |e| *e
        == Event::Notify {
            level: NoticeLevel::Warning,
            message: "heads up".into()
        }));
    assert!(
        saw(&events, |e| *e == Event::Status(None)),
        "an empty status clears it"
    );

    assert!(p.notify("event", json!({"name": "download.completed"})));
    assert!(
        eventually(5, || log_file(tmp.path()).contains("download.completed")).await,
        "the program never got the event"
    );
    p.stop().await;
}

#[tokio::test]
async fn a_call_from_the_program_into_fetchbox_is_answered_as_not_implemented() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    p.request("call-host", json!({}), SEC)
        .await
        .expect("answer");
    assert!(
        eventually(5, || log_file(tmp.path()).contains("not implemented")).await,
        "{}",
        log_file(tmp.path())
    );
    p.stop().await;
}

#[tokio::test]
async fn the_program_gets_a_clean_environment_and_its_own_folders() {
    std::env::set_var("FETCHBOX_PASSWORD", "hunter2");
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _) = start(tmp.path(), "echo", |c| {
        c.env.extra.insert("MINE".into(), "yes".into());
    });
    running(&p).await;
    let vars: Value = p.request("env", json!({}), SEC).await.expect("env");
    assert!(vars.get("FETCHBOX_PASSWORD").is_none());
    assert_eq!(vars["MINE"], "yes");
    assert_eq!(vars["FETCHBOX_PLUGIN_ID"], "demo");
    for d in ["home", "tmp", "state"] {
        assert!(tmp.path().join("data").join(d).is_dir(), "{d}");
    }
    p.stop().await;
}

#[tokio::test]
async fn a_clean_exit_is_not_restarted_but_a_crash_under_never_is_a_failure() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    // `shutdown` makes the stand-in answer and exit with code 0.
    p.request("shutdown", json!({}), SEC).await.ok();
    assert!(
        eventually(10, || p.state() == State::Stopped).await,
        "{:?}",
        p.state()
    );

    let tmp2 = tempfile::tempdir().expect("tmp");
    let (q, _) = start(tmp2.path(), "echo", |c| c.restart = RestartPolicy::Never);
    running(&q).await;
    q.request("crash", json!({}), SEC).await.expect_err("crash");
    assert!(
        eventually(10, || matches!(q.state(), State::Failed(_))).await,
        "{:?}",
        q.state()
    );
}

#[tokio::test]
async fn always_restarts_even_after_a_clean_exit() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, events) = start(tmp.path(), "echo", |c| c.restart = RestartPolicy::Always);
    running(&p).await;
    p.request("shutdown", json!({}), SEC).await.ok();
    assert!(
        eventually(10, || saw(&events, |e| matches!(
            e,
            Event::State(State::Backoff { .. })
        )))
        .await
    );
    running(&p).await;
    p.stop().await;
}

#[tokio::test]
async fn a_stubborn_program_is_ended_after_the_grace_periods() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _) = start(tmp.path(), "ignore-shutdown", |c| {
        c.timing.stop_grace = Duration::from_millis(300);
    });
    running(&p).await;
    let started = std::time::Instant::now();
    p.stop().await;
    assert_eq!(p.state(), State::Stopped);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_whole_process_group_goes_with_the_program() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    let pid = p.request("spawn-child", json!({}), SEC).await.expect("pid")["pid"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .expect("pid");
    assert!(common::alive(pid));
    p.stop().await;
    assert!(
        eventually(5, || !common::alive(pid)).await,
        "the child outlived the plugin"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn dropping_the_handle_stops_the_program() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (p, _) = start(tmp.path(), "echo", |_| {});
    running(&p).await;
    let pid = p.request("spawn-child", json!({}), SEC).await.expect("pid")["pid"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .expect("pid");
    drop(p);
    assert!(
        eventually(8, || !common::alive(pid)).await,
        "the child outlived the handle"
    );
}
