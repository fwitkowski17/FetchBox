//! `fetchbox reset-password`: for a forgotten password. Runs the real binary.

use std::path::Path;
use std::process::{Command, Output};

/// The storage crate is used only to put a password hash in and to look for it again.
struct Db(fetchbox_storage::Connection);

fn db(dir: &Path) -> Db {
    std::fs::create_dir_all(dir).expect("dir");
    Db(fetchbox_storage::open(&dir.join("fetchbox.db")).expect("open"))
}

impl Db {
    fn set_hash(&self, hash: &str) {
        fetchbox_storage::settings::set_password_hash(&self.0, hash).expect("set");
    }

    fn hash(&self) -> Option<String> {
        fetchbox_storage::settings::password_hash(&self.0).expect("get")
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fetchbox"))
        .args(args)
        .env_remove("FETCHBOX_PASSWORD")
        .env_remove("DLM_PASSWORD")
        .output()
        .expect("run fetchbox")
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn removes_the_stored_password_and_says_what_to_do_next() {
    let dir = tempfile::tempdir().expect("tmp");
    let data = dir.path().join("data");
    {
        let d = db(&data);
        d.set_hash("$argon2id$secret");
        assert!(d.hash().is_some());
    }
    // Exactly how the Docker image runs it: its entrypoint passes the other flags first.
    let out = run(&[
        "--data-dir",
        data.to_str().expect("utf8"),
        "--download-dir",
        dir.path().join("dl").to_str().expect("utf8"),
        "--port",
        "8080",
        "reset-password",
    ]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("password removed"), "{}", text(&out));
    assert!(db(&data).hash().is_none());
    // It must not have started the server or created the download directory.
    assert!(!dir.path().join("dl").exists());
}

#[test]
fn is_harmless_when_there_is_nothing_to_reset() {
    let dir = tempfile::tempdir().expect("tmp");
    let data = dir.path().join("data");
    drop(db(&data)); // a database without a password
    let out = run(&["--data-dir", data.to_str().expect("utf8"), "reset-password"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("nothing to reset"), "{}", text(&out));
}

#[test]
fn refuses_a_directory_without_a_database_instead_of_making_one() {
    let dir = tempfile::tempdir().expect("tmp");
    let data = dir.path().join("typo");
    let out = run(&["--data-dir", data.to_str().expect("utf8"), "reset-password"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("no database"), "{}", text(&out));
    assert!(
        !data.exists(),
        "a wrong --data-dir must not leave a directory behind"
    );
}

#[test]
fn an_empty_password_variable_is_not_a_password() {
    // docker compose sets FETCHBOX_PASSWORD to an empty string when it is not configured.
    let dir = tempfile::tempdir().expect("tmp");
    let data = dir.path().join("data");
    db(&data).set_hash("$argon2id$secret");
    let out = Command::new(env!("CARGO_BIN_EXE_fetchbox"))
        .args(["--data-dir", data.to_str().expect("utf8"), "reset-password"])
        .env("FETCHBOX_PASSWORD", "")
        .output()
        .expect("run");
    assert!(out.status.success());
    assert!(!text(&out).contains("always wins"), "{}", text(&out));
}

#[test]
fn warns_that_an_environment_password_wins() {
    let dir = tempfile::tempdir().expect("tmp");
    let data = dir.path().join("data");
    db(&data).set_hash("$argon2id$secret");
    let out = Command::new(env!("CARGO_BIN_EXE_fetchbox"))
        .args(["--data-dir", data.to_str().expect("utf8"), "reset-password"])
        .env("FETCHBOX_PASSWORD", "from-the-environment")
        .output()
        .expect("run");
    assert!(out.status.success());
    assert!(text(&out).contains("always wins"), "{}", text(&out));
}
