//! `fetchbox plugin new` and `plugin list`. Runs the real binary, in a folder of its own.

use std::path::Path;
use std::process::{Command, Output};

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fetchbox"))
        .current_dir(cwd)
        .args(args)
        .env_remove("FETCHBOX_PASSWORD")
        .env_remove("FETCHBOX_DATA_DIR")
        .env_remove("FETCHBOX_DOWNLOAD_DIR")
        .output()
        .expect("run fetchbox")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn makes_a_plugin_in_the_working_folder_and_nothing_else() {
    let cwd = tempfile::tempdir().expect("tmp");
    let out = run(
        cwd.path(),
        &["plugin", "new", "my-hook", "-a", "Filip", "-n", "My hook"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let said = text(&out.stdout);
    assert!(said.contains("Created my-hook"), "{said}");
    assert!(said.contains("plugin.json"), "{said}");

    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(cwd.path().join("my-hook/plugin.json")).expect("plugin.json"),
    )
    .expect("json");
    assert_eq!(manifest["id"], "my-hook");
    assert_eq!(manifest["name"], "My hook");
    assert_eq!(manifest["author"], "Filip");

    // The server's own folders are not made: this command needs none of them.
    let mut entries: Vec<_> = std::fs::read_dir(cwd.path())
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    entries.sort();
    assert_eq!(entries, ["my-hook"]);
}

#[test]
fn a_template_and_a_target_folder_can_be_chosen() {
    let cwd = tempfile::tempdir().expect("tmp");
    let out = run(
        cwd.path(),
        &[
            "plugin",
            "new",
            "page-links",
            "-t",
            "resolver",
            "-o",
            "nested/plugins",
        ],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let dir = cwd.path().join("nested/plugins/page-links");
    assert!(dir.join("plugin.json").is_file());
    assert!(dir.join("bin/probe.py").is_file());
}

#[test]
fn refuses_what_would_overwrite_or_is_not_an_id_or_a_template() {
    let cwd = tempfile::tempdir().expect("tmp");
    assert!(run(cwd.path(), &["plugin", "new", "mine"]).status.success());

    let again = run(cwd.path(), &["plugin", "new", "mine"]);
    assert!(!again.status.success());
    assert!(
        text(&again.stderr).contains("already exists"),
        "{}",
        text(&again.stderr)
    );

    let bad_id = run(cwd.path(), &["plugin", "new", "Bad_Id"]);
    assert!(!bad_id.status.success());
    assert!(text(&bad_id.stderr).contains("not a valid id"));

    let bad_template = run(cwd.path(), &["plugin", "new", "other", "-t", "nope"]);
    assert!(!bad_template.status.success());
    assert!(
        text(&bad_template.stderr).contains("hook"),
        "the error lists the templates"
    );
    assert!(!cwd.path().join("other").exists());
}

#[test]
fn lists_the_templates() {
    let cwd = tempfile::tempdir().expect("tmp");
    let out = run(cwd.path(), &["plugin", "list"]);
    assert!(out.status.success());
    let said = text(&out.stdout);
    for name in ["hook", "process", "resolver"] {
        assert!(said.contains(name), "{said}");
    }
}
