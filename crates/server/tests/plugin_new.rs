//! `fetchbox plugin new`, `plugin list` and `plugin link`. Runs the real binary, in a folder of its
//! own, and builds the Cargo projects it makes.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

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
        &[
            "plugin",
            "new",
            "my-hook",
            "-a",
            "Filip",
            "-n",
            "My hook",
            "-r",
            "filip/my-hook",
        ],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let said = text(&out.stdout);
    assert!(said.contains("Created my-hook"), "{said}");
    assert!(said.contains("plugin.json"), "{said}");
    assert!(
        !said.contains("placeholder"),
        "a repository was given: {said}"
    );

    let manifest: Value = serde_json::from_slice(
        &std::fs::read(cwd.path().join("my-hook/plugin.json")).expect("plugin.json"),
    )
    .expect("json");
    assert_eq!(manifest["id"], "my-hook");
    assert_eq!(manifest["name"], "My hook");
    assert_eq!(manifest["author"], "Filip");
    assert_eq!(manifest["homepage"], "https://github.com/filip/my-hook");

    // The server's own folders are not made: this command needs none of them.
    let mut entries: Vec<_> = std::fs::read_dir(cwd.path())
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    entries.sort();
    assert_eq!(entries, ["my-hook"]);
}

#[test]
fn without_a_repository_the_placeholder_is_pointed_out() {
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
    assert!(
        text(&out.stdout).contains("OWNER/<id>"),
        "{}",
        text(&out.stdout)
    );

    let dir = cwd.path().join("nested/plugins/page-links");
    for file in [
        "plugin.json",
        "Cargo.toml",
        "src/main.rs",
        ".gitignore",
        ".github/workflows/release.yml",
    ] {
        assert!(dir.join(file).is_file(), "{file}");
    }
    let manifest = std::fs::read_to_string(dir.join("plugin.json")).expect("plugin.json");
    assert!(
        manifest.contains("\"repo\": \"OWNER/page-links\""),
        "{manifest}"
    );
}

#[test]
fn refuses_what_would_overwrite_or_is_not_an_id_a_template_or_a_repository() {
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

    let bad_repo = run(cwd.path(), &["plugin", "new", "other", "-r", "nobody"]);
    assert!(!bad_repo.status.success());
    assert!(text(&bad_repo.stderr).contains("owner/name"));
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
    assert!(!said.contains("_rust"), "{said}");
}

// ---- the Cargo projects the templates make --------------------------------------------------

/// A shared target folder next to the one `cargo test` uses, so the projects of one run (and of the
/// next) build their dependencies once.
fn target_dir() -> PathBuf {
    let exe = Path::new(env!("CARGO_BIN_EXE_fetchbox"));
    exe.ancestors()
        .nth(2)
        .expect("the target folder")
        .join("plugin-templates")
}

fn cargo(project: &Path, args: &[&str]) {
    // `cargo fmt` takes no --offline; everything else must not reach for the network. The flag goes
    // before a `--` that hands the rest to a tool.
    let mut full = vec![args[0]];
    if args[0] != "fmt" {
        full.push("--offline");
    }
    full.extend_from_slice(&args[1..]);
    let out = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(project)
        .args(&full)
        .env("CARGO_TARGET_DIR", target_dir())
        .output()
        .expect("run cargo");
    assert!(
        out.status.success(),
        "cargo {args:?} in {}:\n{}{}",
        project.display(),
        text(&out.stdout),
        text(&out.stderr)
    );
}

/// Scaffold `template` as `id` in `cwd`, check what the project's own CI checks (formatting, clippy,
/// its tests) and build it.
fn build_template(cwd: &Path, id: &str, template: &str) -> (PathBuf, PathBuf) {
    let made = run(
        cwd,
        &[
            "plugin",
            "new",
            id,
            "-t",
            template,
            "-r",
            "someone/demo",
            "-a",
            "Tester",
        ],
    );
    assert!(made.status.success(), "{}", text(&made.stderr));
    let project = cwd.join(id);
    cargo(&project, &["fmt", "--check"]);
    cargo(
        &project,
        &["clippy", "--quiet", "--all-targets", "--", "-D", "warnings"],
    );
    cargo(&project, &["test", "--quiet"]);
    cargo(&project, &["build", "--quiet"]);
    let program = target_dir()
        .join("debug")
        .join(format!("{id}{}", std::env::consts::EXE_SUFFIX));
    assert!(program.is_file(), "{} was not built", program.display());
    (project, program)
}

#[test]
fn the_process_template_builds_and_speaks_the_protocol() {
    let cwd = tempfile::tempdir().expect("tmp");
    // "test" is an id that would clash with a crate name without the prefix of the package.
    let (_, program) = build_template(cwd.path(), "test-process", "process");

    let mut child = Command::new(&program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the program");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
    let mut ask = |message: Value| {
        writeln!(stdin, "{message}").expect("write");
        stdin.flush().expect("flush");
    };

    ask(json!({"id": 1, "method": "initialize", "params": {"settings": {"greeting": "hi"}}}));
    let first: Value =
        serde_json::from_str(&lines.next().expect("a line").expect("read")).expect("json");
    assert_eq!(first, json!({"id": 1, "result": {}}));
    let status: Value =
        serde_json::from_str(&lines.next().expect("a line").expect("read")).expect("json");
    assert_eq!(status["method"], "status");
    let _started = lines.next().expect("the log line").expect("read");

    ask(json!({"id": 2, "method": "invoke", "params": {"action": "ping"}}));
    let ping: Value =
        serde_json::from_str(&lines.next().expect("a line").expect("read")).expect("json");
    assert_eq!(ping["result"]["message"], "hi");

    ask(json!({"id": 3, "method": "shutdown"}));
    let bye: Value =
        serde_json::from_str(&lines.next().expect("a line").expect("read")).expect("json");
    assert_eq!(bye["id"], 3);
    drop(stdin);
    assert!(child.wait().expect("exit").success());
}

#[test]
fn the_resolver_template_builds_and_lists_a_range() {
    let cwd = tempfile::tempdir().expect("tmp");
    let (_, program) = build_template(cwd.path(), "test-resolver", "resolver");

    let out = Command::new(&program)
        .args(["https://example.com/img[08-10].jpg", "100"])
        .output()
        .expect("run the program");
    assert!(out.status.success(), "{}", text(&out.stderr));
    let json: Value = serde_json::from_slice(&out.stdout).expect("JSON on standard output");
    let urls: Vec<&str> = json["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["url"].as_str().expect("url"))
        .collect();
    assert_eq!(
        urls,
        [
            "https://example.com/img08.jpg",
            "https://example.com/img09.jpg",
            "https://example.com/img10.jpg"
        ]
    );

    let bad = Command::new(&program)
        .arg("https://example.com/plain.zip")
        .output()
        .expect("run the program");
    assert_eq!(bad.status.code(), Some(1));
    assert!(
        text(&bad.stderr).contains("no range"),
        "{}",
        text(&bad.stderr)
    );
}

// ---- plugin link ------------------------------------------------------------------------------

#[test]
fn link_puts_the_manifest_and_the_built_program_where_the_plugin_will_look() {
    let cwd = tempfile::tempdir().expect("tmp");
    let data = cwd.path().join("data");
    let project = cwd.path().join("linked");
    assert!(run(
        cwd.path(),
        &[
            "plugin",
            "new",
            "linked",
            "-t",
            "process",
            "-r",
            "me/linked"
        ]
    )
    .status
    .success());

    let link = |args: &[&str]| {
        let mut all = vec!["--data-dir", data.to_str().expect("utf-8")];
        all.extend_from_slice(args);
        run(cwd.path(), &all)
    };

    // A data directory that does not exist is a mistake, not something to create.
    let missing = link(&["plugin", "link", project.to_str().expect("utf-8")]);
    assert!(!missing.status.success());
    assert!(
        text(&missing.stderr).contains("no data directory"),
        "{}",
        text(&missing.stderr)
    );
    assert!(!data.exists());

    // So is a program that was not built.
    std::fs::create_dir_all(&data).expect("data dir");
    let unbuilt = link(&["plugin", "link", project.to_str().expect("utf-8")]);
    assert!(!unbuilt.status.success());
    assert!(
        text(&unbuilt.stderr).contains("build it first"),
        "{}",
        text(&unbuilt.stderr)
    );
    assert!(
        !data.join("plugins").exists(),
        "nothing is installed when the program is missing"
    );

    // Any file stands in for the program here; the build itself is tested above.
    let program = project.join("target/release/linked");
    std::fs::create_dir_all(program.parent().expect("parent")).expect("target dir");
    std::fs::write(&program, b"a build").expect("program");
    let linked = link(&["plugin", "link", project.to_str().expect("utf-8")]);
    assert!(linked.status.success(), "{}", text(&linked.stderr));
    assert!(
        text(&linked.stdout).contains("Linked linked"),
        "{}",
        text(&linked.stdout)
    );

    assert_eq!(
        std::fs::read(data.join("plugins/linked/plugin.json")).expect("manifest"),
        std::fs::read(project.join("plugin.json")).expect("source manifest")
    );
    let dep = data.join("plugin-data/linked/deps/linked");
    assert_eq!(
        std::fs::read(dep.join("linked")).expect("program"),
        b"a build"
    );
    let record: Value = serde_json::from_slice(
        &std::fs::read(dep.join(".fetchbox-dependency.json")).expect("record"),
    )
    .expect("json");
    assert_eq!(record["host"], "local");

    // A rebuild is seen without linking again.
    std::fs::write(&program, b"a newer build").expect("rebuild");
    assert_eq!(
        std::fs::read(dep.join("linked")).expect("program"),
        b"a newer build"
    );

    // The project of a plugin without a program has only the manifest to link.
    assert!(run(cwd.path(), &["plugin", "new", "plain"])
        .status
        .success());
    let plain = link(&[
        "plugin",
        "link",
        cwd.path().join("plain").to_str().expect("utf-8"),
    ]);
    assert!(plain.status.success(), "{}", text(&plain.stderr));
    assert!(data.join("plugins/plain/plugin.json").is_file());
    assert!(!data.join("plugin-data/plain").exists());
}
