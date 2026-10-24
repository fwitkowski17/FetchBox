//! `fetchbox plugin new` and `fetchbox plugin list`: start an extension (SPEC 6.15) from a template.
//!
//! The templates are the folders of `plugins/templates/`, embedded at compile time like the
//! plugins that ship, so this works from any installation (package, container, a build from
//! source) without a checkout. `__ID__`, `__NAME__` and `__AUTHOR__` are replaced; inside a
//! `.json` file the value is escaped as a JSON string, elsewhere it is written as it is. The
//! manifest is read with the same code the server uses before anything is written, so what this
//! makes is a plugin that validates.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use fetchbox_core::plugin::is_valid_id;
use fetchbox_plugins::install;
use rust_embed::RustEmbed;

/// The starting points: one folder each, with a `plugin.json`.
#[derive(RustEmbed)]
#[folder = "../../plugins/templates"]
struct Templates;

/// The template used when none is named.
pub const DEFAULT_TEMPLATE: &str = "hook";

/// What `plugin new` was asked for.
#[derive(Debug, Clone)]
pub struct NewPlugin {
    pub id: String,
    pub template: String,
    /// Display name; the id in words when absent.
    pub name: Option<String>,
    /// Author; `git config user.name` when absent.
    pub author: Option<String>,
    /// The folder `<id>/` is created in.
    pub out: PathBuf,
}

/// What `plugin new` made.
#[derive(Debug)]
pub struct Created {
    pub dir: PathBuf,
    /// Paths inside `dir`, sorted.
    pub files: Vec<String>,
}

/// The names of the templates, sorted.
pub fn template_names() -> Vec<String> {
    let mut names: Vec<String> = Templates::iter()
        .filter_map(|path| path.split_once('/').map(|(folder, _)| folder.to_owned()))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// What a template makes, in a line (the description of its manifest).
fn describe(template: &str) -> String {
    Templates::get(&format!("{template}/plugin.json"))
        .and_then(|f| serde_json::from_slice::<serde_json::Value>(&f.data).ok())
        .and_then(|m| m["description"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The id in words: `page-links` is `Page Links`.
fn words(id: &str) -> String {
    id.split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The author when none is given: the git user, if git is there and knows one.
fn git_user() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (out.status.success() && !name.is_empty()).then_some(name)
}

/// `value` as the inside of a JSON string.
fn json_escaped(value: &str) -> anyhow::Result<String> {
    let quoted = serde_json::to_string(value).context("escape a value for JSON")?;
    Ok(quoted
        .strip_prefix('"')
        .and_then(|q| q.strip_suffix('"'))
        .unwrap_or(&quoted)
        .to_owned())
}

/// One file of the new plugin, ready to be written.
struct Rendered {
    path: String,
    content: String,
    executable: bool,
}

fn render(template: &str, id: &str, name: &str, author: &str) -> anyhow::Result<Vec<Rendered>> {
    let prefix = format!("{template}/");
    let mut files = Vec::new();
    for full in Templates::iter() {
        let Some(path) = full.strip_prefix(&prefix) else {
            continue;
        };
        let file = Templates::get(&full).with_context(|| format!("read template file {full}"))?;
        let text = std::str::from_utf8(&file.data)
            .with_context(|| format!("template file {full} is not text"))?;
        let (id, name, author) = if path.ends_with(".json") {
            (
                json_escaped(id)?,
                json_escaped(name)?,
                json_escaped(author)?,
            )
        } else {
            (id.to_owned(), name.to_owned(), author.to_owned())
        };
        files.push(Rendered {
            path: path.to_owned(),
            content: text
                .replace("__ID__", &id)
                .replace("__NAME__", &name)
                .replace("__AUTHOR__", &author),
            // Embedded files carry no mode: what is in `bin/` is a program.
            executable: path.starts_with("bin/"),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    anyhow::ensure!(
        !files.is_empty(),
        "no template \"{template}\" (one of: {})",
        template_names().join(", ")
    );
    Ok(files)
}

/// Make the folder `<out>/<id>/` from a template. Nothing is written when the id or the manifest
/// is invalid or the folder exists.
pub fn create(request: &NewPlugin) -> anyhow::Result<Created> {
    let id = request.id.as_str();
    anyhow::ensure!(
        is_valid_id(id),
        "\"{id}\" is not a valid id (2 to 32 lowercase letters, digits and \"-\", starting with a letter)"
    );
    let name = request.name.clone().unwrap_or_else(|| words(id));
    let author = request
        .author
        .clone()
        .or_else(git_user)
        .unwrap_or_else(|| "Your Name".to_owned());

    let files = render(&request.template, id, &name, &author)?;
    let manifest = files
        .iter()
        .find(|f| f.path == "plugin.json")
        .with_context(|| format!("template \"{}\" has no plugin.json", request.template))?;
    let parsed = install::parse_manifest(manifest.content.as_bytes())
        .map_err(|e| anyhow::anyhow!("the name or author is not usable: {e}"))?;
    anyhow::ensure!(
        parsed.id == id,
        "the template makes the id \"{}\"",
        parsed.id
    );

    // `-o .` is the default; "my-hook" reads better than "./my-hook".
    let dir = if request.out == Path::new(".") {
        PathBuf::from(id)
    } else {
        request.out.join(id)
    };
    std::fs::create_dir_all(&request.out)
        .with_context(|| format!("create {}", request.out.display()))?;
    // Not `create_dir_all`: an existing folder is somebody's work.
    std::fs::create_dir(&dir).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            anyhow::anyhow!("{} already exists", dir.display())
        } else {
            anyhow::Error::new(e).context(format!("create {}", dir.display()))
        }
    })?;
    if let Err(e) = write_all(&dir, &files) {
        // Do not leave half a plugin behind; the folder was ours (it did not exist).
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(Created {
        dir,
        files: files.into_iter().map(|f| f.path).collect(),
    })
}

fn write_all(dir: &Path, files: &[Rendered]) -> anyhow::Result<()> {
    for file in files {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        std::fs::write(&path, &file.content)
            .with_context(|| format!("write {}", path.display()))?;
        #[cfg(unix)]
        if file.executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .with_context(|| format!("make {} executable", path.display()))?;
        }
    }
    Ok(())
}

/// `fetchbox plugin new ...`: make the folder and say what to do next.
pub fn run_new(request: &NewPlugin) -> anyhow::Result<()> {
    let created = create(request)?;
    let dir = created.dir.display();
    let mut text = format!("Created {dir} (template: {})\n\nFiles:\n", request.template);
    for file in &created.files {
        text.push_str(&format!("  {file}\n"));
    }
    text.push_str(&format!(
        "\nNext:\n  1. Put the folder inside <data dir>/plugins/ (or create it there with -o)\n  \
         2. Open Extensions, press Rescan, review the permissions, approve and switch it on\n  \
         3. Read {dir}/README.md, then plugins/README.md for everything the manifest can do\n"
    ));
    print_out(&text)
}

/// `fetchbox plugin list`.
pub fn run_list() -> anyhow::Result<()> {
    let mut text = String::new();
    for name in template_names() {
        text.push_str(&format!("  {name:<9} {}\n", describe(&name)));
    }
    print_out(&text)
}

/// Output meant for the person at the terminal, not a log line.
fn print_out(text: &str) -> anyhow::Result<()> {
    std::io::stdout()
        .lock()
        .write_all(text.as_bytes())
        .context("write to standard output")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(dir: &Path, id: &str, template: &str) -> NewPlugin {
        NewPlugin {
            id: id.into(),
            template: template.into(),
            name: Some("Fish & Chips / Demo".into()),
            author: Some("A|B & C".into()),
            out: dir.to_path_buf(),
        }
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).expect("dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                files.extend(walk(&path));
            } else {
                files.push(path);
            }
        }
        files
    }

    #[test]
    fn the_templates_are_the_three_starting_points() {
        assert_eq!(template_names(), ["hook", "process", "resolver"]);
        assert!(template_names().iter().any(|n| n == DEFAULT_TEMPLATE));
        for name in template_names() {
            assert!(!describe(&name).is_empty(), "{name} has no description");
        }
    }

    #[test]
    fn every_template_makes_a_valid_plugin() {
        let out = tempfile::tempdir().expect("temp dir");
        for template in template_names() {
            let id = format!("demo-{template}");
            let created = create(&request(out.path(), &id, &template)).expect("created");
            assert_eq!(created.dir, out.path().join(&id));
            assert!(created.files.iter().any(|f| f == "plugin.json"));
            assert!(created.files.iter().any(|f| f == "README.md"));

            let manifest = std::fs::read(created.dir.join("plugin.json")).expect("plugin.json");
            let m = install::parse_manifest(&manifest).expect("a valid manifest");
            assert_eq!(m.id, id);
            assert_eq!(m.name, "Fish & Chips / Demo");
            assert_eq!(m.author, "A|B & C");

            for path in walk(&created.dir) {
                let text = std::fs::read_to_string(&path).expect("text file");
                assert!(
                    !["__ID__", "__NAME__", "__AUTHOR__"]
                        .iter()
                        .any(|p| text.contains(p)),
                    "{} still has a placeholder",
                    path.display()
                );
                #[cfg(unix)]
                if path.parent().is_some_and(|p| p.ends_with("bin")) {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(&path)
                        .expect("metadata")
                        .permissions()
                        .mode();
                    assert!(mode & 0o111 != 0, "{} is not executable", path.display());
                }
            }
        }
    }

    #[test]
    fn a_name_with_quotes_is_escaped_in_json_and_plain_in_text() {
        let out = tempfile::tempdir().expect("temp dir");
        let mut r = request(out.path(), "quoted", "hook");
        r.name = Some(r#"Say "hi" \ there"#.into());
        let created = create(&r).expect("created");

        let manifest = std::fs::read(created.dir.join("plugin.json")).expect("plugin.json");
        let m = install::parse_manifest(&manifest).expect("valid");
        assert_eq!(m.name, r#"Say "hi" \ there"#);
        let readme = std::fs::read_to_string(created.dir.join("README.md")).expect("README");
        assert!(readme.starts_with(r#"# Say "hi" \ there"#));
    }

    #[test]
    fn the_defaults_come_from_the_id() {
        assert_eq!(words("page-links"), "Page Links");
        assert_eq!(words("ab"), "Ab");

        let out = tempfile::tempdir().expect("temp dir");
        let mut r = request(out.path(), "page-links", "hook");
        r.name = None;
        let created = create(&r).expect("created");
        let manifest = std::fs::read(created.dir.join("plugin.json")).expect("plugin.json");
        assert_eq!(
            install::parse_manifest(&manifest).expect("valid").name,
            "Page Links"
        );
    }

    #[test]
    fn nothing_is_overwritten_and_nothing_is_left_after_a_refusal() {
        let out = tempfile::tempdir().expect("temp dir");
        create(&request(out.path(), "mine", "hook")).expect("first");
        let marker = out.path().join("mine/plugin.json");
        std::fs::write(&marker, "my work").expect("edit");

        let again = create(&request(out.path(), "mine", "hook")).expect_err("exists");
        assert!(again.to_string().contains("already exists"), "{again}");
        assert_eq!(std::fs::read_to_string(&marker).expect("read"), "my work");

        // Ids that are not ids, including ones that would climb out of the folder.
        for bad in ["Bad_Id", "x", "../escape", "a/b", ""] {
            let err = create(&request(out.path(), bad, "hook")).expect_err(bad);
            assert!(err.to_string().contains("not a valid id"), "{bad}: {err}");
        }
        // A name the manifest refuses is caught before anything is written.
        let mut r = request(out.path(), "no-name", "hook");
        r.name = Some("x".repeat(200));
        assert!(create(&r).is_err());
        assert!(!out.path().join("no-name").exists());

        let mut entries: Vec<_> = std::fs::read_dir(out.path())
            .expect("dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        entries.sort();
        assert_eq!(entries, ["mine"]);
    }

    #[test]
    fn an_unknown_template_is_refused() {
        let out = tempfile::tempdir().expect("temp dir");
        let err = create(&request(out.path(), "demo", "nope")).expect_err("unknown");
        assert!(err.to_string().contains("no template"), "{err}");
        assert!(!out.path().join("demo").exists());
    }
}
