//! `fetchbox plugin new` and `fetchbox plugin list`: start an extension (SPEC 6.15) from a template.
//!
//! A template is the starting point of a project that lives in a repository of its own: the
//! manifest, the program if there is one (a Cargo project), a README, and the GitHub workflows that
//! test it and publish its releases. The templates are the folders of `plugins/templates/`, embedded
//! at compile time like the plugins that ship, so this works from any installation (package,
//! container, a build from source) without a checkout. A folder whose name starts with `_` is not a
//! template but files that several templates share (`RUST_TEMPLATES` use `_rust`).
//!
//! `__ID__`, `__NAME__`, `__AUTHOR__` and `__REPO__` are replaced; inside a `.json` file the value
//! is escaped as a JSON string, elsewhere it is written as it is (so only the id, which is
//! restricted to `[a-z0-9-]`, may appear in `Cargo.toml` and the workflows). The manifest is read
//! with the same code the server uses before anything is written, so what this makes is a plugin
//! that validates.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use fetchbox_core::plugin::is_valid_id;
use fetchbox_plugins::install;
use rust_embed::RustEmbed;

/// The starting points: one folder each, with a `plugin.json`; folders with a `_` are shared files.
#[derive(RustEmbed)]
#[folder = "../../plugins/templates"]
struct Templates;

/// The template used when none is named.
pub const DEFAULT_TEMPLATE: &str = "hook";

/// The templates that are a Cargo project: they also get the files of the folder `_rust`.
const RUST_TEMPLATES: &[&str] = &["process", "resolver"];

/// What `plugin new` was asked for.
#[derive(Debug, Clone)]
pub struct NewPlugin {
    pub id: String,
    pub template: String,
    /// Display name; the id in words when absent.
    pub name: Option<String>,
    /// Author; `git config user.name` when absent.
    pub author: Option<String>,
    /// The GitHub repository (`owner/name`) that will hold the project and its releases; a
    /// placeholder that validates (`OWNER/<id>`) when absent.
    pub repo: Option<String>,
    /// The folder `<id>/` is created in.
    pub out: PathBuf,
}

/// What `plugin new` made.
#[derive(Debug)]
pub struct Created {
    pub dir: PathBuf,
    /// Paths inside `dir`, sorted.
    pub files: Vec<String>,
    /// No repository was given, so `plugin.json` has a placeholder to replace.
    pub placeholder_repo: bool,
}

/// The names of the templates, sorted.
pub fn template_names() -> Vec<String> {
    let mut names: Vec<String> = Templates::iter()
        .filter_map(|path| path.split_once('/').map(|(folder, _)| folder.to_owned()))
        .filter(|folder| !folder.starts_with('_'))
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

/// Is `repo` an `owner/name` of GitHub? (The rule `validate` applies to `github.repo`.)
fn is_repo(repo: &str) -> bool {
    let part = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    };
    repo.split_once('/')
        .is_some_and(|(owner, name)| part(owner) && part(name))
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

/// The values that replace the placeholders.
struct Values<'a> {
    id: &'a str,
    name: &'a str,
    author: &'a str,
    repo: &'a str,
}

impl Values<'_> {
    fn fill(&self, text: &str, json: bool) -> anyhow::Result<String> {
        let value = |v: &str| {
            if json {
                json_escaped(v)
            } else {
                Ok(v.to_owned())
            }
        };
        Ok(text
            .replace("__ID__", &value(self.id)?)
            .replace("__NAME__", &value(self.name)?)
            .replace("__AUTHOR__", &value(self.author)?)
            .replace("__REPO__", &value(self.repo)?))
    }
}

/// One file of the new plugin, ready to be written.
struct Rendered {
    path: String,
    content: String,
}

/// The files of `folder` (a template or a shared folder), with their paths inside it.
fn folder_files(folder: &str) -> Vec<(String, String)> {
    let prefix = format!("{folder}/");
    let mut files = Vec::new();
    for full in Templates::iter() {
        let Some(path) = full.strip_prefix(&prefix) else {
            continue;
        };
        if let Some(file) = Templates::get(&full) {
            if let Ok(text) = std::str::from_utf8(&file.data) {
                files.push((path.to_owned(), text.to_owned()));
            }
        }
    }
    files
}

fn render(template: &str, values: &Values<'_>) -> anyhow::Result<Vec<Rendered>> {
    // Not `_rust` either: that folder is only the files that templates share.
    anyhow::ensure!(
        template_names().iter().any(|n| n == template),
        "no template \"{template}\" (one of: {})",
        template_names().join(", ")
    );
    let mut sources = folder_files(template);
    if RUST_TEMPLATES.contains(&template) {
        // A template's own file wins over a shared one of the same name.
        for (path, text) in folder_files("_rust") {
            if !sources.iter().any(|(p, _)| *p == path) {
                sources.push((path, text));
            }
        }
    }
    let mut files = Vec::new();
    for (path, text) in sources {
        files.push(Rendered {
            content: values.fill(&text, path.ends_with(".json"))?,
            path,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
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
    let placeholder_repo = request.repo.is_none();
    let repo = request
        .repo
        .clone()
        .unwrap_or_else(|| format!("OWNER/{id}"));
    anyhow::ensure!(
        is_repo(&repo),
        "\"{repo}\" is not a GitHub repository like owner/name"
    );

    let files = render(
        &request.template,
        &Values {
            id,
            name: &name,
            author: &author,
            repo: &repo,
        },
    )?;
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
        placeholder_repo,
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
        "\nNext:\n  1. Read {dir}/README.md: how to try it, and how to publish it from a repository of its own\n  \
         2. Try it here: fetchbox --data-dir <data dir> plugin link {dir}\n  \
         3. Then Extensions -> Rescan, review the permissions, approve and switch it on\n"
    ));
    if created.placeholder_repo {
        text.push_str(
            "\nNote: the GitHub repository is the placeholder OWNER/<id> (in plugin.json: homepage, and\n\
             the dependency's github.repo if there is one). Set it before the first release, or pass --repo.\n",
        );
    }
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
pub(crate) fn print_out(text: &str) -> anyhow::Result<()> {
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
            repo: Some("someone/demo-repo".into()),
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
    fn every_template_makes_a_valid_plugin_with_the_files_of_a_repository() {
        let out = tempfile::tempdir().expect("temp dir");
        for template in template_names() {
            let id = format!("demo-{template}");
            let created = create(&request(out.path(), &id, &template)).expect("created");
            assert_eq!(created.dir, out.path().join(&id));
            assert!(!created.placeholder_repo);

            // What a repository of its own needs, hidden files included.
            for needed in [
                "plugin.json",
                "README.md",
                ".gitignore",
                ".github/workflows/ci.yml",
                ".github/workflows/release.yml",
            ] {
                assert!(
                    created.files.iter().any(|f| f == needed),
                    "{template}: no {needed} in {:?}",
                    created.files
                );
                assert!(created.dir.join(needed).is_file(), "{template}: {needed}");
            }
            let is_rust = RUST_TEMPLATES.contains(&template.as_str());
            assert_eq!(
                created.dir.join("Cargo.toml").is_file(),
                is_rust,
                "{template}"
            );
            assert_eq!(
                created.dir.join("src/main.rs").is_file(),
                is_rust,
                "{template}"
            );

            let manifest = std::fs::read(created.dir.join("plugin.json")).expect("plugin.json");
            let m = install::parse_manifest(&manifest).expect("a valid manifest");
            assert_eq!(m.id, id);
            assert_eq!(m.name, "Fish & Chips / Demo");
            assert_eq!(m.author, "A|B & C");
            assert_eq!(
                m.homepage.as_deref(),
                Some("https://github.com/someone/demo-repo")
            );
            // A program is a dependency that follows the releases of the repository.
            for dep in &m.dependencies {
                let github = dep.github.as_ref().expect("a GitHub dependency");
                assert_eq!(github.repo, "someone/demo-repo");
                assert_eq!(
                    github.assets.len(),
                    m.platforms.len(),
                    "an asset per platform"
                );
                for platform in &m.platforms {
                    assert_eq!(
                        github.assets.get(platform).map(String::as_str),
                        Some(format!("{id}-{platform}").as_str())
                    );
                }
            }
            assert_eq!(m.dependencies.len(), usize::from(is_rust));

            for path in walk(&created.dir) {
                let text = std::fs::read_to_string(&path).expect("text file");
                assert!(
                    !["__ID__", "__NAME__", "__AUTHOR__", "__REPO__"]
                        .iter()
                        .any(|p| text.contains(p)),
                    "{} still has a placeholder",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn a_cargo_project_names_its_program_after_the_id_and_cannot_be_claimed_by_a_workspace() {
        let out = tempfile::tempdir().expect("temp dir");
        let created = create(&request(out.path(), "test", "process")).expect("created");
        let cargo = std::fs::read_to_string(created.dir.join("Cargo.toml")).expect("Cargo.toml");
        assert!(cargo.contains("name = \"fetchbox-plugin-test\""), "{cargo}");
        assert!(cargo.contains("[[bin]]\nname = \"test\""), "{cargo}");
        assert!(cargo.contains("\n[workspace]\n"), "{cargo}");
        // Only the id (restricted to [a-z0-9-]) goes into files that are not JSON or Markdown.
        for file in ["Cargo.toml", ".github/workflows/release.yml"] {
            let text = std::fs::read_to_string(created.dir.join(file)).expect(file);
            assert!(!text.contains("Fish") && !text.contains("A|B"), "{file}");
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
        let mut r = request(out.path(), "page-links", "process");
        r.name = None;
        r.repo = None;
        let created = create(&r).expect("created");
        assert!(created.placeholder_repo);
        let manifest = std::fs::read(created.dir.join("plugin.json")).expect("plugin.json");
        let m = install::parse_manifest(&manifest).expect("valid");
        assert_eq!(m.name, "Page Links");
        assert_eq!(
            m.dependencies[0].github.as_ref().map(|g| g.repo.as_str()),
            Some("OWNER/page-links")
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
        // A repository that is not owner/name, and a name the manifest refuses.
        for bad in ["nobody", "a/b/c", "has space/x", "/x", "x/"] {
            let mut r = request(out.path(), "no-repo", "hook");
            r.repo = Some(bad.into());
            let err = create(&r).expect_err(bad);
            assert!(err.to_string().contains("owner/name"), "{bad}: {err}");
        }
        let mut r = request(out.path(), "no-name", "hook");
        r.name = Some("x".repeat(200));
        assert!(create(&r).is_err());

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
        for template in ["nope", "_rust"] {
            let err = create(&request(out.path(), "demo", template)).expect_err("unknown");
            assert!(
                err.to_string().contains("one of: hook, process, resolver"),
                "{err}"
            );
        }
        assert!(!out.path().join("demo").exists());
    }
}
