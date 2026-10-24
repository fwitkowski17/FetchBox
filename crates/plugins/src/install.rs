//! A plugin's files on disk (SPEC 6.15).
//!
//! ```text
//! <data>/plugins/<id>/              the plugin, as installed (written once, never by the host again)
//! <data>/plugins/.staging/<token>/  a package being reviewed before it is installed
//! <data>/plugin-data/<id>/          what the plugin may write: dependencies, temp, home, state
//! ```
//!
//! Installing is staging (unpack and read the manifest, nothing runs), then a review by the user,
//! then an atomic rename into place. Everything is synchronous: call it from `spawn_blocking`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use fetchbox_core::plugin::{self, Manifest};
use sha2::{Digest, Sha256};

use crate::archive::{self, Limits};
use crate::{Error, Result};

/// The manifest's file name, at the top of a plugin.
pub const MANIFEST_FILE: &str = "plugin.json";

/// Where plugins live below the data directory.
#[derive(Debug, Clone)]
pub struct Layout {
    data_dir: PathBuf,
}

impl Layout {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Layout {
            data_dir: data_dir.into(),
        }
    }

    /// `<data>/plugins`: one folder per installed plugin.
    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }

    fn staging_dir(&self) -> PathBuf {
        self.plugins_dir().join(".staging")
    }

    /// `<data>/plugins/<id>`.
    pub fn plugin_dir(&self, id: &str) -> PathBuf {
        self.plugins_dir().join(id)
    }

    /// `<data>/plugin-programs`: the programs of FetchBox's list that plugins share, one folder per
    /// program and version (`<program>/<version>/`).
    pub fn programs_dir(&self) -> PathBuf {
        self.data_dir.join("plugin-programs")
    }

    /// `<data>/plugin-data/<id>`: the only place a plugin's programs are meant to write.
    pub fn data_dir_of(&self, id: &str) -> PathBuf {
        self.data_dir.join("plugin-data").join(id)
    }
}

impl Layout {
    /// A fresh hidden file inside the staging folder, to write an uploaded ZIP to before it is
    /// unpacked (swept with the old staged packages if it is left behind).
    pub fn upload_path(&self) -> Result<PathBuf> {
        let dir = self.staging_dir();
        fs::create_dir_all(&dir)?;
        Ok(dir.join(format!(".upload-{}.zip", new_token()?)))
    }
}

/// SHA-256 of `bytes`, as lowercase hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Read a manifest: its size, its JSON and its rules (`fetchbox_core::plugin::validate`).
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest> {
    if bytes.len() > plugin::MAX_MANIFEST_BYTES {
        return Err(Error::Manifest(format!(
            "{MANIFEST_FILE} is larger than {} KiB",
            plugin::MAX_MANIFEST_BYTES / 1024
        )));
    }
    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|e| Error::Manifest(format!("{MANIFEST_FILE} is not valid: {e}")))?;
    plugin::validate(&manifest).map_err(|e| Error::Manifest(format!("{MANIFEST_FILE}: {e}")))?;
    Ok(manifest)
}

/// Read and check `dir/plugin.json`. Returns the manifest and the hash of its bytes.
pub fn read_manifest(dir: &Path) -> Result<(Manifest, String)> {
    let path = dir.join(MANIFEST_FILE);
    let meta = fs::metadata(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::Manifest(format!("{MANIFEST_FILE} is missing")),
        _ => Error::Io(e),
    })?;
    if !meta.is_file() {
        return Err(Error::Manifest(format!("{MANIFEST_FILE} is not a file")));
    }
    if meta.len() > plugin::MAX_MANIFEST_BYTES as u64 {
        return Err(Error::Manifest(format!(
            "{MANIFEST_FILE} is larger than {} KiB",
            plugin::MAX_MANIFEST_BYTES / 1024
        )));
    }
    let bytes = fs::read(&path)?;
    Ok((parse_manifest(&bytes)?, sha256_hex(&bytes)))
}

/// A package that was unpacked and read but is not installed: nothing of it has run.
#[derive(Debug, Clone)]
pub struct Staged {
    /// 32 hexadecimal characters; names the staging folder.
    pub token: String,
    pub manifest: Manifest,
    /// SHA-256 of the `plugin.json` that was read.
    pub manifest_hash: String,
    root: PathBuf,
}

impl Staged {
    /// The folder that holds `plugin.json`.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// A token is what a client sends back, so it is checked before it becomes part of a path.
fn valid_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn new_token() -> Result<String> {
    let mut raw = [0u8; 16];
    getrandom::fill(&mut raw).map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    Ok(hex(&raw))
}

/// Start a staged package: an empty folder `…/.staging/<token>/pkg` to unpack or write into.
/// Returns the token and that folder; finish with [`finish_staged`].
pub fn new_staging(layout: &Layout) -> Result<(String, PathBuf)> {
    let token = new_token()?;
    let pkg = layout.staging_dir().join(&token).join("pkg");
    fs::create_dir_all(&pkg)?;
    Ok((token, pkg))
}

/// Where `plugin.json` is in an unpacked package: at its top, or in its only top-level folder (how
/// most archivers pack a folder). Mac junk is ignored.
fn find_root(pkg: &Path) -> Result<PathBuf> {
    if pkg.join(MANIFEST_FILE).is_file() {
        return Ok(pkg.to_path_buf());
    }
    let mut dirs = Vec::new();
    let mut others = 0;
    for entry in fs::read_dir(pkg)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "__MACOSX" || name == ".DS_Store" {
            continue;
        }
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        } else {
            others += 1;
        }
    }
    match dirs.as_slice() {
        [only] if others == 0 && only.join(MANIFEST_FILE).is_file() => Ok(only.clone()),
        _ => Err(Error::Manifest(format!(
            "{MANIFEST_FILE} not found: it must be at the top of the ZIP or in its single top folder"
        ))),
    }
}

/// Read the manifest of what was put into the staging folder of `token`.
pub fn finish_staged(layout: &Layout, token: &str) -> Result<Staged> {
    let staged = load_staged(layout, token);
    if staged.is_err() {
        // Nothing usable: do not leave the files around.
        cancel(layout, token);
    }
    staged
}

/// Open a staged package again (the user confirms the install in a second request).
pub fn load_staged(layout: &Layout, token: &str) -> Result<Staged> {
    if !valid_token(token) {
        return Err(Error::NotFound("unknown package".into()));
    }
    let pkg = layout.staging_dir().join(token).join("pkg");
    if !pkg.is_dir() {
        return Err(Error::NotFound(
            "that package is gone (it is kept for a short while only): add it again".into(),
        ));
    }
    let root = find_root(&pkg)?;
    let (manifest, manifest_hash) = read_manifest(&root)?;
    Ok(Staged {
        token: token.to_owned(),
        manifest,
        manifest_hash,
        root,
    })
}

/// Unpack a plugin's ZIP into a new staging folder and read its manifest. The ZIP file itself stays
/// where it is (the caller owns it).
pub fn stage_zip(layout: &Layout, zip: &Path) -> Result<Staged> {
    let size = fs::metadata(zip)?.len();
    if size > plugin::MAX_PLUGIN_ZIP as u64 {
        return Err(Error::Archive(format!(
            "the ZIP is larger than {} MiB",
            plugin::MAX_PLUGIN_ZIP / (1024 * 1024)
        )));
    }
    let (token, pkg) = new_staging(layout)?;
    if let Err(e) = archive::extract_zip(zip, &pkg, 0, &Limits::default()) {
        cancel(layout, &token);
        return Err(e);
    }
    finish_staged(layout, &token)
}

/// Throw a staged package away. Quiet when it is already gone.
pub fn cancel(layout: &Layout, token: &str) {
    if valid_token(token) {
        let _ = fs::remove_dir_all(layout.staging_dir().join(token));
    }
}

/// Delete staged packages older than `max_age` (the user went away without deciding).
pub fn sweep_staging(layout: &Layout, max_age: Duration) {
    let Ok(entries) = fs::read_dir(layout.staging_dir()) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > max_age);
        if old {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// What [`commit`] did.
#[derive(Debug, Clone)]
pub struct Installed {
    pub manifest: Manifest,
    pub manifest_hash: String,
    /// The version it replaced, if the plugin was installed before.
    pub replaced: Option<String>,
}

/// Move a staged package into `plugins/<id>/`, replacing an older version of the same plugin. The
/// swap is two renames inside one folder, so a crash leaves either the old or the new plugin (and
/// at worst a stale `.staging/` folder, swept later), never half of each.
pub fn commit(layout: &Layout, token: &str) -> Result<Installed> {
    let staged = load_staged(layout, token)?;
    let id = staged.manifest.id.clone();
    let target = layout.plugin_dir(&id);
    let stash = layout.staging_dir().join(token).join("previous");
    let replaced = if target.exists() {
        let version = read_manifest(&target).ok().map(|(m, _)| m.version);
        fs::rename(&target, &stash)?;
        Some(version.unwrap_or_default())
    } else {
        None
    };
    if let Err(e) = fs::rename(staged.root(), &target) {
        if replaced.is_some() {
            let _ = fs::rename(&stash, &target);
        }
        return Err(Error::Io(e));
    }
    cancel(layout, token);
    Ok(Installed {
        manifest: staged.manifest,
        manifest_hash: staged.manifest_hash,
        replaced,
    })
}

/// Delete an installed plugin's files, and with `keep_data` false what it had written.
pub fn remove(layout: &Layout, id: &str, keep_data: bool) -> Result<()> {
    if !plugin::is_valid_id(id) {
        return Err(Error::NotFound(format!("no plugin \"{id}\"")));
    }
    let dir = layout.plugin_dir(id);
    if !dir.is_dir() {
        return Err(Error::NotFound(format!("no plugin \"{id}\"")));
    }
    fs::remove_dir_all(&dir)?;
    if !keep_data {
        match fs::remove_dir_all(layout.data_dir_of(id)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Io(e)),
        }
    }
    Ok(())
}

/// A folder found in the plugins directory.
#[derive(Debug, Clone)]
pub struct Discovered {
    /// The folder's name.
    pub folder: String,
    /// Its manifest and the hash of it, or why it cannot be used.
    pub result: std::result::Result<(Manifest, String), String>,
}

/// Look at every folder of the plugins directory (hidden ones, such as `.staging`, are skipped). A
/// folder whose manifest is unusable, or whose name is not the plugin's id, is reported rather than
/// left out, so the user can see why it does not show up as a plugin.
pub fn discover(layout: &Layout) -> Vec<Discovered> {
    let Ok(entries) = fs::read_dir(layout.plugins_dir()) else {
        return Vec::new();
    };
    let mut found: Vec<Discovered> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let folder = e.file_name().to_string_lossy().into_owned();
            if folder.starts_with('.') {
                return None;
            }
            let result = read_manifest(&e.path())
                .map_err(|err| err.to_string())
                .and_then(|(m, hash)| {
                    if m.id == folder {
                        Ok((m, hash))
                    } else {
                        Err(format!(
                            "the folder is called \"{folder}\" but the plugin's id is \"{}\"",
                            m.id
                        ))
                    }
                });
            Some(Discovered { folder, result })
        })
        .collect();
    found.sort_by(|a, b| a.folder.cmp(&b.folder));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    const MANIFEST: &str = r#"{"api_version":1,"id":"demo","name":"Demo","version":"1.0.0"}"#;

    fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, body) in files {
            w.start_file(*name, opts).expect("file");
            w.write_all(body.as_bytes()).expect("write");
        }
        w.finish().expect("finish").into_inner()
    }

    fn setup(files: &[(&str, &str)]) -> (tempfile::TempDir, Layout, PathBuf) {
        let tmp = tempfile::tempdir().expect("tmp");
        let layout = Layout::new(tmp.path().join("data"));
        let zip = tmp.path().join("p.zip");
        fs::write(&zip, zip_with(files)).expect("write");
        (tmp, layout, zip)
    }

    #[test]
    fn a_zip_with_the_manifest_at_the_top_is_staged() {
        let (_tmp, layout, zip) = setup(&[("plugin.json", MANIFEST), ("bin/run", "x")]);
        let s = stage_zip(&layout, &zip).expect("staged");
        assert_eq!(s.manifest.id, "demo");
        assert_eq!(s.manifest_hash, sha256_hex(MANIFEST.as_bytes()));
        assert!(s.root().join("bin/run").is_file());
        assert!(valid_token(&s.token));
    }

    #[test]
    fn a_zip_with_one_top_folder_is_staged_from_inside_it() {
        let (_tmp, layout, zip) = setup(&[
            ("demo-1.0/plugin.json", MANIFEST),
            ("demo-1.0/bin/run", "x"),
            ("__MACOSX/demo-1.0/._plugin.json", "junk"),
        ]);
        let s = stage_zip(&layout, &zip).expect("staged");
        assert!(s.root().ends_with("demo-1.0"));
        assert!(s.root().join("bin/run").is_file());
    }

    #[test]
    fn a_zip_without_a_usable_manifest_is_refused_and_leaves_nothing() {
        let cases: [(&[(&str, &str)], &str); 5] = [
            (&[("readme.txt", "hi")], "not found"),
            (&[("a/plugin.json", MANIFEST), ("b/x", "y")], "not found"),
            (&[("plugin.json", "{ not json")], "not valid"),
            (
                &[(
                    "plugin.json",
                    r#"{"api_version":1,"id":"../x","name":"X","version":"1"}"#,
                )],
                "valid id",
            ),
            (
                &[(
                    "plugin.json",
                    r#"{"api_version":9,"id":"demo","name":"X","version":"1"}"#,
                )],
                "update FetchBox",
            ),
        ];
        for (files, expect) in cases {
            let (_tmp, layout, zip) = setup(files);
            let err = stage_zip(&layout, &zip).expect_err(expect);
            assert!(matches!(err, Error::Manifest(_)), "{err}");
            assert!(err.to_string().contains(expect), "{err}");
            let left = fs::read_dir(layout.staging_dir())
                .map(|d| d.count())
                .unwrap_or(0);
            assert_eq!(left, 0, "a refused package is not kept");
        }
    }

    #[test]
    fn an_unsafe_zip_is_refused_and_leaves_nothing() {
        let (_tmp, layout, zip) = setup(&[("plugin.json", MANIFEST), ("../evil", "x")]);
        let err = stage_zip(&layout, &zip).expect_err("unsafe");
        assert!(matches!(err, Error::Archive(_)), "{err}");
        assert_eq!(fs::read_dir(layout.staging_dir()).expect("dir").count(), 0);
    }

    #[test]
    fn an_oversized_manifest_is_refused() {
        let big = format!(
            r#"{{"api_version":1,"id":"demo","name":"D","version":"1","description":"{}"}}"#,
            "x".repeat(plugin::MAX_MANIFEST_BYTES)
        );
        let err = parse_manifest(big.as_bytes()).expect_err("big");
        assert!(err.to_string().contains("larger"), "{err}");
    }

    #[test]
    fn a_staged_package_is_installed_by_its_token() {
        let (_tmp, layout, zip) = setup(&[("plugin.json", MANIFEST), ("bin/run", "x")]);
        let s = stage_zip(&layout, &zip).expect("staged");
        let done = commit(&layout, &s.token).expect("commit");
        assert_eq!(done.manifest.id, "demo");
        assert_eq!(done.replaced, None);
        assert!(layout.plugin_dir("demo").join("bin/run").is_file());
        assert!(
            !layout.staging_dir().join(&s.token).exists(),
            "staging is cleaned up"
        );
        // A token works once.
        assert!(matches!(commit(&layout, &s.token), Err(Error::NotFound(_))));
    }

    #[test]
    fn an_update_replaces_the_old_files_completely() {
        let (tmp, layout, zip) = setup(&[("plugin.json", MANIFEST), ("old-only", "x")]);
        let s = stage_zip(&layout, &zip).expect("staged");
        commit(&layout, &s.token).expect("commit");

        let newer = MANIFEST.replace("1.0.0", "1.1.0");
        let zip2 = tmp.path().join("p2.zip");
        fs::write(
            &zip2,
            zip_with(&[("plugin.json", &newer), ("new-only", "y")]),
        )
        .expect("write");
        let s2 = stage_zip(&layout, &zip2).expect("staged");
        let done = commit(&layout, &s2.token).expect("commit");
        assert_eq!(done.replaced.as_deref(), Some("1.0.0"));
        let dir = layout.plugin_dir("demo");
        assert!(dir.join("new-only").is_file());
        assert!(
            !dir.join("old-only").exists(),
            "nothing of the old version is left"
        );
        assert_eq!(read_manifest(&dir).expect("manifest").0.version, "1.1.0");
    }

    #[test]
    fn a_token_cannot_name_another_path() {
        let (_tmp, layout, _zip) = setup(&[]);
        for t in [
            "../..",
            "",
            "abc",
            "A".repeat(32).as_str(),
            "../../../../etc/passwd",
        ] {
            assert!(
                matches!(load_staged(&layout, t), Err(Error::NotFound(_))),
                "{t:?}"
            );
            cancel(&layout, t);
        }
        assert!(matches!(
            load_staged(&layout, &"0".repeat(32)),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn cancelling_removes_the_staged_files() {
        let (_tmp, layout, zip) = setup(&[("plugin.json", MANIFEST)]);
        let s = stage_zip(&layout, &zip).expect("staged");
        cancel(&layout, &s.token);
        assert!(matches!(
            load_staged(&layout, &s.token),
            Err(Error::NotFound(_))
        ));
        cancel(&layout, &s.token);
    }

    #[test]
    fn old_staged_packages_are_swept() {
        let (_tmp, layout, zip) = setup(&[("plugin.json", MANIFEST)]);
        let s = stage_zip(&layout, &zip).expect("staged");
        sweep_staging(&layout, Duration::from_secs(3600));
        assert!(load_staged(&layout, &s.token).is_ok(), "a fresh one stays");
        sweep_staging(&layout, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(20));
        sweep_staging(&layout, Duration::ZERO);
        assert!(matches!(
            load_staged(&layout, &s.token),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn removing_a_plugin_can_keep_what_it_wrote() {
        let (_tmp, layout, zip) = setup(&[("plugin.json", MANIFEST)]);
        let s = stage_zip(&layout, &zip).expect("staged");
        commit(&layout, &s.token).expect("commit");
        let data = layout.data_dir_of("demo");
        fs::create_dir_all(data.join("deps")).expect("data");
        remove(&layout, "demo", true).expect("remove");
        assert!(!layout.plugin_dir("demo").exists());
        assert!(data.exists(), "kept");
        assert!(matches!(
            remove(&layout, "demo", false),
            Err(Error::NotFound(_))
        ));

        let s = stage_zip(&layout, &zip).expect("staged");
        commit(&layout, &s.token).expect("commit");
        remove(&layout, "demo", false).expect("remove");
        assert!(!data.exists(), "gone with the plugin");
        assert!(matches!(
            remove(&layout, "../x", false),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn folders_in_the_plugins_directory_are_discovered_or_explained() {
        let tmp = tempfile::tempdir().expect("tmp");
        let layout = Layout::new(tmp.path());
        let plugins = layout.plugins_dir();
        for (folder, manifest) in [
            ("demo", Some(MANIFEST)),
            ("renamed", Some(MANIFEST)),
            ("broken", Some("{")),
            ("empty", None),
            (".staging", Some(MANIFEST)),
        ] {
            fs::create_dir_all(plugins.join(folder)).expect("dir");
            if let Some(m) = manifest {
                fs::write(plugins.join(folder).join(MANIFEST_FILE), m).expect("write");
            }
        }
        fs::write(plugins.join("stray-file"), "x").expect("file");
        let found = discover(&layout);
        let names: Vec<_> = found.iter().map(|d| d.folder.as_str()).collect();
        assert_eq!(names, ["broken", "demo", "empty", "renamed"]);
        let by = |n: &str| found.iter().find(|d| d.folder == n).expect("found");
        assert!(by("demo").result.is_ok());
        assert!(by("renamed")
            .result
            .as_ref()
            .expect_err("renamed")
            .contains("called \"renamed\""));
        assert!(by("broken")
            .result
            .as_ref()
            .expect_err("broken")
            .contains("not valid"));
        assert!(by("empty")
            .result
            .as_ref()
            .expect_err("empty")
            .contains("missing"));
    }

    #[test]
    fn nothing_is_discovered_before_any_plugin_exists() {
        let tmp = tempfile::tempdir().expect("tmp");
        assert!(discover(&Layout::new(tmp.path())).is_empty());
    }
}
