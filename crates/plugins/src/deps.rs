//! A plugin's dependencies: programs the manifest names and FetchBox downloads for it (SPEC 6.15).
//!
//! A dependency is fetched into `<data>/plugin-data/<plugin>/deps/<id>/` and used from there. The
//! download is verified against a SHA-256 the manifest states (or the release's checksum file, for a
//! GitHub dependency) before anything is unpacked, unpacked beside the final place and swapped in
//! with a rename, so a failed or interrupted install leaves the old version (or nothing) behind.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fetchbox_core::plugin::{
    shared_programs, ArchiveKind, Dependency, GithubDependency, SharedProgram,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::archive::{self, Limits};
use crate::fetch::Fetcher;
use crate::install::{hex, Layout};
use crate::{Error, Result};

/// Name of the file that records what was installed, inside the dependency's folder.
const INFO_FILE: &str = ".fetchbox-dependency.json";
/// The `version` recorded for a program that [`link_local`] put in place.
const LOCAL_BUILD: &str = "local build";
/// Largest download when the manifest or the release does not say how big the file is.
const DEFAULT_MAX: u64 = 500 * 1024 * 1024;
const API_TIMEOUT: Duration = Duration::from_secs(20);

/// What is known about an installed dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepInfo {
    /// The release it was fetched at (a tag), when it came from GitHub.
    pub version: Option<String>,
    /// SHA-256 of the file that was downloaded.
    pub sha256: String,
    /// Where it came from.
    pub host: String,
    /// Unix seconds.
    pub installed_at: i64,
}

/// How far a download is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub dependency: String,
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// What to download for this platform.
#[derive(Debug)]
struct Resolved {
    url: String,
    sha256: String,
    archive: ArchiveKind,
    strip: u32,
    max: u64,
    version: Option<String>,
}

fn cap(declared: Option<u64>) -> u64 {
    declared.map_or(DEFAULT_MAX, |s| {
        s.saturating_add(s / 10).saturating_add(1024 * 1024)
    })
}

/// Guess how an asset is packed from its name.
fn archive_of(name: &str) -> ArchiveKind {
    let n = name.to_ascii_lowercase();
    if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
        ArchiveKind::TarGz
    } else if n.ends_with(".tar.xz") || n.ends_with(".txz") {
        ArchiveKind::TarXz
    } else if n.ends_with(".zip") {
        ArchiveKind::Zip
    } else if n.ends_with(".gz") {
        ArchiveKind::Gz
    } else {
        ArchiveKind::Raw
    }
}

/// The digest listed for `asset` in a checksum file (`<hex>  <name>` or `<hex> *<name>` per line, as
/// `sha256sum` and `SHA2-256SUMS` write them).
pub fn checksum_for(list: &str, asset: &str) -> Option<String> {
    list.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == asset && digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_hexdigit()))
            .then(|| digest.to_ascii_lowercase())
    })
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

/// Installs the dependencies of plugins, one at a time.
#[derive(Debug)]
pub struct Installer {
    fetcher: Fetcher,
    layout: Layout,
    platform: String,
    /// The programs plugins may share (FetchBox's list; tests put their own here).
    shared: Vec<SharedProgram>,
    gate: Semaphore,
}

impl Installer {
    pub fn new(fetcher: Fetcher, layout: Layout) -> Self {
        Self::for_platform(fetcher, layout, fetchbox_core::plugin::platform_key())
    }

    /// Like [`new`](Self::new) for another platform than the running one (tests).
    pub fn for_platform(fetcher: Fetcher, layout: Layout, platform: String) -> Self {
        Installer {
            fetcher,
            layout,
            platform,
            shared: shared_programs().to_vec(),
            gate: Semaphore::new(1),
        }
    }

    /// Use another list of shared programs than FetchBox's own (tests).
    pub fn with_shared_programs(mut self, list: Vec<SharedProgram>) -> Self {
        self.shared = list;
        self
    }

    fn deps_dir(&self, plugin: &str) -> PathBuf {
        self.layout.data_dir_of(plugin).join("deps")
    }

    /// The entry of the list that a shared dependency names.
    fn shared_entry(&self, dep: &Dependency) -> Option<&SharedProgram> {
        let name = dep.shared.as_deref()?;
        self.shared.iter().find(|p| p.id == name)
    }

    /// The dependency as the installer works with it: a shared one is the entry of the list (its
    /// files and the name of its program), under the plugin's own id and label.
    fn effective(&self, dep: &Dependency) -> Dependency {
        let Some(entry) = self.shared_entry(dep) else {
            return dep.clone();
        };
        Dependency {
            id: dep.id.clone(),
            label: if dep.label.is_empty() {
                entry.label.clone()
            } else {
                dep.label.clone()
            },
            optional: dep.optional,
            bin: None,
            platforms: entry.platforms.clone(),
            github: None,
            shared: None,
        }
    }

    /// The folder of a dependency: its own below the plugin's data, or, for a shared program, the
    /// one every plugin that names it uses, `plugin-programs/<program>/<version>/` (a new version
    /// is a new folder).
    pub fn dep_dir(&self, plugin: &str, dep: &Dependency) -> PathBuf {
        match self.shared_entry(dep) {
            Some(entry) => self
                .layout
                .programs_dir()
                .join(&entry.id)
                .join(&entry.version),
            None => self.deps_dir(plugin).join(&dep.id),
        }
    }

    /// The dependency's program.
    pub fn bin_path(&self, plugin: &str, dep: &Dependency) -> PathBuf {
        let effective = self.effective(dep);
        self.dep_dir(plugin, dep)
            .join(program_name_for(&effective, &self.platform))
    }

    /// What is installed, if the dependency is in place (its record is there and so is its program).
    ///
    /// A dependency that follows a GitHub release by `tag` is in place only as that release: the
    /// folder of a plugin's data survives an update of the plugin, so the program of the version
    /// before it is still there and must not be taken for the one the new manifest names. A program
    /// that was linked from a local build stands in for any tag.
    pub fn state(&self, plugin: &str, dep: &Dependency) -> Option<DepInfo> {
        if !self.bin_path(plugin, dep).is_file() {
            return None;
        }
        let raw = std::fs::read(self.dep_dir(plugin, dep).join(INFO_FILE)).ok()?;
        let info: DepInfo = serde_json::from_slice(&raw).ok()?;
        if info.version.as_deref() != Some(LOCAL_BUILD) {
            let github = dep.github.as_ref();
            if let Some(tag) = github.and_then(|g| g.tag.as_deref()) {
                if info.version.as_deref() != Some(tag) {
                    return None;
                }
            }
            // The manifest of an update can keep the release and still name other programs (a
            // rebuilt asset): what is installed must be the file the manifest pins.
            if let Some(want) = github.and_then(|g| g.sha256.get(&self.platform)) {
                if !info.sha256.eq_ignore_ascii_case(want) {
                    return None;
                }
            }
        }
        Some(info)
    }

    async fn github_json(&self, url: &str) -> Result<serde_json::Value> {
        let body = self
            .fetcher
            .get_bytes(
                url,
                2 * 1024 * 1024,
                API_TIMEOUT,
                Some("application/vnd.github+json"),
            )
            .await?;
        serde_json::from_slice(&body)
            .map_err(|_| Error::Fetch("GitHub's answer is not JSON".into()))
    }

    async fn resolve(&self, dep: &Dependency) -> Result<Resolved> {
        if let Some(src) = dep.platforms.get(&self.platform) {
            return Ok(Resolved {
                url: src.url.clone(),
                sha256: src.sha256.to_ascii_lowercase(),
                archive: src.archive,
                strip: src.strip,
                max: cap(src.size),
                version: None,
            });
        }
        let Some(g) = &dep.github else {
            return Err(Error::Fetch(format!(
                "{} is not available for {}",
                dep.id, self.platform
            )));
        };
        self.resolve_github(dep, g).await
    }

    async fn resolve_github(&self, dep: &Dependency, g: &GithubDependency) -> Result<Resolved> {
        let Some(asset) = g.assets.get(&self.platform) else {
            return Err(Error::Fetch(format!(
                "{} has no release file for {}",
                dep.id, self.platform
            )));
        };
        let url = match &g.tag {
            Some(tag) => format!(
                "{}/repos/{}/releases/tags/{tag}",
                self.fetcher.github_api(),
                g.repo
            ),
            None => format!(
                "{}/repos/{}/releases/latest",
                self.fetcher.github_api(),
                g.repo
            ),
        };
        let release = self.github_json(&url).await?;
        let tag = release["tag_name"].as_str().map(str::to_owned);
        let find = |name: &str| {
            release["assets"].as_array().and_then(|list| {
                list.iter()
                    .find(|a| a["name"].as_str() == Some(name))
                    .map(|a| {
                        (
                            a["browser_download_url"].as_str().map(str::to_owned),
                            a["size"].as_u64(),
                        )
                    })
            })
        };
        let release_name = tag.as_deref().unwrap_or("the release");
        let (Some(asset_url), size) = find(asset).unwrap_or((None, None)) else {
            return Err(Error::Fetch(format!(
                "{release_name} of {} has no file \"{asset}\"",
                g.repo
            )));
        };
        // A hash pinned in the manifest is the stronger word: the release's own checksum file is
        // published by whoever could replace the asset.
        if let Some(pinned) = g.sha256.get(&self.platform) {
            return Ok(Resolved {
                url: asset_url,
                sha256: pinned.to_ascii_lowercase(),
                archive: archive_of(asset),
                strip: 0,
                max: cap(size),
                version: tag,
            });
        }
        let Some(sums_name) = &g.sha256_asset else {
            return Err(Error::Fetch(format!(
                "{} lists no checksum file, so its download cannot be verified",
                dep.id
            )));
        };
        let Some((Some(sums_url), _)) = find(sums_name) else {
            return Err(Error::Fetch(format!(
                "{release_name} of {} has no checksum file \"{sums_name}\"",
                g.repo
            )));
        };
        let sums = self
            .fetcher
            .get_bytes(&sums_url, 1024 * 1024, API_TIMEOUT, None)
            .await?;
        let sums = String::from_utf8_lossy(&sums);
        let Some(sha256) = checksum_for(&sums, asset) else {
            return Err(Error::Fetch(format!(
                "\"{sums_name}\" does not list a SHA-256 for \"{asset}\""
            )));
        };
        Ok(Resolved {
            url: asset_url,
            sha256,
            archive: archive_of(asset),
            strip: 0,
            max: cap(size),
            version: tag,
        })
    }

    /// Download, verify, unpack and put in place one dependency of a plugin. Waits for other installs
    /// first (one at a time keeps a small machine usable). `progress` is called while it downloads.
    ///
    /// A shared program that is in place when its turn comes (another plugin fetched it while this
    /// one waited) is not fetched again.
    pub async fn install(
        &self,
        plugin: &str,
        dep: &Dependency,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<DepInfo> {
        let _turn = self
            .gate
            .acquire()
            .await
            .map_err(|_| Error::Fetch("the installer is shut down".into()))?;
        let entry = match (&dep.shared, self.shared_entry(dep)) {
            (Some(name), None) => {
                return Err(Error::NotFound(format!(
                    "FetchBox has no shared program \"{name}\""
                )));
            }
            (_, entry) => entry,
        };
        if entry.is_some() {
            if let Some(info) = self.state(plugin, dep) {
                return Ok(info);
            }
        }

        let effective = self.effective(dep);
        let mut resolved = self.resolve(&effective).await?;
        if let Some(entry) = entry {
            resolved.version = Some(entry.version.clone());
        }
        let target = self.dep_dir(plugin, dep);
        // The work folder is next to the final one, so the last step is a rename on one file system.
        let parent = target
            .parent()
            .ok_or_else(|| Error::Fetch("a dependency has no folder".into()))?;
        tokio::fs::create_dir_all(parent).await?;
        let work = parent.join(format!(".tmp-{}", hex(&random()?)));
        tokio::fs::create_dir_all(&work).await?;
        let result = self
            .install_in(&effective, &target, &resolved, &work, progress)
            .await;
        let _ = tokio::fs::remove_dir_all(&work).await;
        let info = result?;
        if let Some(entry) = entry {
            self.prune_other_versions(entry);
            // The copy this plugin kept in its own folder before the program was shared is unused now.
            let _ = std::fs::remove_dir_all(self.deps_dir(plugin).join(&dep.id));
        }
        tracing::info!(
            target: "activity",
            plugin,
            dependency = %dep.id,
            shared = dep.shared.as_deref().unwrap_or("-"),
            version = info.version.as_deref().unwrap_or("-"),
            host = %info.host,
            "plugin dependency installed"
        );
        Ok(info)
    }

    /// Was this shared program installed before, in a version that is not the current one? That is
    /// the case for a user who chose to download an optional program and then got a FetchBox whose
    /// list names a newer version, and for one who had it in the plugin's own folder before the
    /// manifest took it from the list. They are not asked again: it is fetched anew.
    pub fn was_installed_before(&self, plugin: &str, dep: &Dependency) -> bool {
        let Some(entry) = self.shared_entry(dep) else {
            return false;
        };
        if self
            .deps_dir(plugin)
            .join(&dep.id)
            .join(INFO_FILE)
            .is_file()
        {
            return true;
        }
        let Ok(versions) = std::fs::read_dir(self.layout.programs_dir().join(&entry.id)) else {
            return false;
        };
        versions.flatten().any(|v| {
            let name = v.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.')
                && name != entry.version.as_str()
                && v.path().join(INFO_FILE).is_file()
        })
    }

    /// Every plugin resolves a shared program to the version of FetchBox's list, so the folders of
    /// the versions before it are not wanted by anybody (a program that is still running from one
    /// keeps running; it is unlinked, not killed). Best effort.
    fn prune_other_versions(&self, entry: &SharedProgram) {
        let Ok(versions) = std::fs::read_dir(self.layout.programs_dir().join(&entry.id)) else {
            return;
        };
        for version in versions.flatten() {
            let name = version.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == entry.version.as_str() {
                continue;
            }
            let _ = std::fs::remove_dir_all(version.path());
        }
    }

    async fn install_in(
        &self,
        dep: &Dependency,
        target: &Path,
        r: &Resolved,
        work: &Path,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<DepInfo> {
        let file = work.join("download");
        let id = dep.id.clone();
        let report = move |downloaded, total| {
            progress(Progress {
                dependency: id.clone(),
                downloaded,
                total,
            });
        };
        let got = self.fetcher.download(&r.url, &file, r.max, &report).await?;
        if got != r.sha256 {
            return Err(Error::Fetch(format!(
                "the checksum of the {} download does not match (expected {}, got {got})",
                dep.id, r.sha256
            )));
        }

        let out = work.join("out");
        let bin = program_name_for(dep, &self.platform).to_owned();
        let (kind, strip) = (r.archive, r.strip);
        let (file2, out2, bin2) = (file.clone(), out.clone(), bin.clone());
        tokio::task::spawn_blocking(move || {
            archive::extract(kind, &file2, &out2, &bin2, strip, &Limits::default())
        })
        .await
        .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))??;

        let program = archive::entry_path(&out, &bin, 0, &Limits::default())?
            .ok_or_else(|| Error::Fetch(format!("\"{bin}\" is not a usable program path")))?;
        if !program.is_file() {
            return Err(Error::Fetch(format!(
                "the {} download does not contain \"{bin}\"",
                dep.id
            )));
        }
        make_executable(&program)?;

        let info = DepInfo {
            version: r.version.clone(),
            sha256: r.sha256.clone(),
            host: fetchbox_core::plugin::host_of(&r.url).unwrap_or_default(),
            installed_at: now(),
        };
        let json = serde_json::to_vec_pretty(&info)
            .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
        tokio::fs::write(out.join(INFO_FILE), json).await?;

        // Swap it in: the old folder moves aside first, and comes back if the new one cannot.
        let old = work.join("previous");
        let had_old = tokio::fs::try_exists(target).await?;
        if had_old {
            tokio::fs::rename(target, &old).await?;
        }
        if let Err(e) = tokio::fs::rename(&out, target).await {
            if had_old {
                let _ = tokio::fs::rename(&old, target).await;
            }
            return Err(Error::Io(e));
        }
        Ok(info)
    }
}

/// Where the program is inside a dependency's folder on `platform`.
fn program_name_for<'a>(dep: &'a Dependency, platform: &str) -> &'a str {
    dep.platforms
        .get(platform)
        .and_then(|s| s.bin.as_deref())
        .or(dep.bin.as_deref())
        .unwrap_or(&dep.id)
}

/// Use a program built on this machine as the dependency `dep` of `plugin`, instead of downloading
/// it (development of a plugin, `fetchbox plugin link`): the program is linked into the dependency's
/// folder (copied where links are not available) and recorded as installed, so enabling the plugin
/// does not fetch anything. Whatever was installed there before is replaced. The link follows
/// rebuilds of `program`; the plugin's own restart picks the new build up.
pub fn link_local(
    layout: &Layout,
    plugin: &str,
    dep: &Dependency,
    program: &Path,
) -> Result<DepInfo> {
    if let Some(name) = &dep.shared {
        return Err(Error::Fetch(format!(
            "\"{}\" is the shared program \"{name}\" of FetchBox's list: it is installed once for every plugin and cannot be replaced by a local build",
            dep.id
        )));
    }
    let platform = fetchbox_core::plugin::platform_key();
    let name = program_name_for(dep, &platform);
    let dir = layout.data_dir_of(plugin).join("deps").join(&dep.id);
    let target = archive::entry_path(&dir, name, 0, &Limits::default())?
        .ok_or_else(|| Error::Fetch(format!("\"{name}\" is not a usable program path")))?;
    let program = program
        .canonicalize()
        .map_err(|e| Error::NotFound(format!("no program at {}: {e}", program.display())))?;
    if !program.is_file() {
        return Err(Error::NotFound(format!(
            "{} is not a file",
            program.display()
        )));
    }

    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&program, &target)?;
    #[cfg(not(unix))]
    std::fs::copy(&program, &target).map(|_| ())?;

    let info = DepInfo {
        version: Some(LOCAL_BUILD.into()),
        sha256: String::new(),
        host: "local".into(),
        installed_at: now(),
    };
    let json = serde_json::to_vec_pretty(&info)
        .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    std::fs::write(dir.join(INFO_FILE), json)?;
    Ok(info)
}

fn random() -> Result<[u8; 6]> {
    let mut raw = [0u8; 6];
    getrandom::fill(&mut raw).map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::routing::get;
    use axum::Router;
    use fetchbox_core::plugin::{DepSource, Dependency, GithubDependency};
    use fetchbox_http_engine::UrlPolicy;

    fn sha(bytes: &[u8]) -> String {
        crate::install::sha256_hex(bytes)
    }

    async fn serve(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move { axum::serve(listener, app).await });
        format!("http://{addr}")
    }

    fn installer(tmp: &Path, base_api: Option<&str>) -> Installer {
        let mut f = Fetcher::new(UrlPolicy::default())
            .expect("fetcher")
            .allow_plain_http();
        if let Some(api) = base_api {
            f = f.with_github_api(api);
        }
        Installer::for_platform(f, Layout::new(tmp), "linux-x86_64".into())
    }

    fn pinned(id: &str, url: String, body: &[u8], archive: ArchiveKind, strip: u32) -> Dependency {
        let mut platforms = BTreeMap::new();
        platforms.insert(
            "linux-x86_64".to_owned(),
            DepSource {
                url,
                sha256: sha(body),
                archive,
                strip,
                size: Some(body.len() as u64),
                bin: None,
            },
        );
        Dependency {
            id: id.into(),
            label: String::new(),
            optional: false,
            bin: None,
            platforms,
            github: None,
            shared: None,
        }
    }

    fn nothing(_: Progress) {}

    fn targz(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (name, body, mode) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(*mode);
            h.set_cksum();
            tar.append_data(&mut h, name, *body).expect("append");
        }
        let raw = tar.into_inner().expect("tar");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw).expect("gz");
        gz.finish().expect("finish")
    }

    #[test]
    fn a_dependency_pinned_to_a_release_is_in_place_only_as_that_release() {
        let tmp = tempfile::tempdir().expect("tmp");
        let layout = Layout::new(tmp.path());
        let installer = Installer::new(
            Fetcher::new(UrlPolicy::default()).expect("fetcher"),
            layout.clone(),
        );
        let dep = |tag: Option<&str>| Dependency {
            id: "tool".into(),
            label: String::new(),
            optional: false,
            bin: None,
            platforms: BTreeMap::new(),
            github: Some(GithubDependency {
                repo: "me/tool".into(),
                assets: BTreeMap::new(),
                sha256_asset: Some("SUMS".into()),
                tag: tag.map(str::to_owned),
                sha256: BTreeMap::new(),
            }),
            shared: None,
        };
        // What an install of release v1.0.0 leaves behind.
        let folder = layout.data_dir_of("demo").join("deps/tool");
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("tool"), b"v1").expect("program");
        let record = |version: Option<&str>| DepInfo {
            version: version.map(str::to_owned),
            sha256: String::new(),
            host: "github.com".into(),
            installed_at: 0,
        };
        let write = |info: &DepInfo| {
            std::fs::write(
                folder.join(INFO_FILE),
                serde_json::to_vec(info).expect("json"),
            )
            .expect("record");
        };

        write(&record(Some("v1.0.0")));
        assert!(
            installer.state("demo", &dep(None)).is_some(),
            "no tag: any release will do"
        );
        assert!(installer.state("demo", &dep(Some("v1.0.0"))).is_some());
        assert!(
            installer.state("demo", &dep(Some("v2.0.0"))).is_none(),
            "the manifest of an update names another release"
        );
        write(&record(None));
        assert!(installer.state("demo", &dep(Some("v1.0.0"))).is_none());
        assert!(installer.state("demo", &dep(None)).is_some());
        write(&record(Some(LOCAL_BUILD)));
        assert!(
            installer.state("demo", &dep(Some("v2.0.0"))).is_some(),
            "a local build stands in"
        );
    }

    fn shared_entry(version: &str, url: String, body: &[u8]) -> SharedProgram {
        let mut platforms = BTreeMap::new();
        platforms.insert(
            "linux-x86_64".to_owned(),
            DepSource {
                url,
                sha256: sha(body),
                archive: ArchiveKind::Raw,
                strip: 0,
                size: Some(body.len() as u64),
                bin: Some("bin/ffmpeg".into()),
            },
        );
        SharedProgram {
            id: "ffmpeg".into(),
            label: "ffmpeg for the tests".into(),
            version: version.into(),
            platforms,
        }
    }

    fn shared_dep(id: &str) -> Dependency {
        Dependency {
            id: id.into(),
            label: String::new(),
            optional: true,
            bin: None,
            platforms: BTreeMap::new(),
            github: None,
            shared: Some("ffmpeg".into()),
        }
    }

    #[tokio::test]
    async fn a_shared_program_is_downloaded_once_for_every_plugin_that_names_it() {
        let hits = Arc::new(AtomicU64::new(0));
        let counted = hits.clone();
        let base = serve(Router::new().route(
            "/ff",
            get(move || {
                counted.fetch_add(1, Ordering::SeqCst);
                async { b"static ffmpeg".to_vec() }
            }),
        ))
        .await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), None).with_shared_programs(vec![shared_entry(
            "1",
            format!("{base}/ff"),
            b"static ffmpeg",
        )]);

        // Two plugins switched on together: one waits for the other's download and finds it there.
        let (a, b) = (shared_dep("ffmpeg"), shared_dep("tools"));
        let (first, second) = tokio::join!(
            inst.install("alpha", &a, &nothing),
            inst.install("beta", &b, &nothing)
        );
        let (first, second) = (first.expect("alpha"), second.expect("beta"));
        assert_eq!(hits.load(Ordering::SeqCst), 1, "fetched once");
        assert_eq!(first.version.as_deref(), Some("1"));
        assert_eq!(second.version.as_deref(), Some("1"));

        // The program is shared, whatever each plugin calls the dependency, and is not in the
        // data folder of either.
        let program = tmp.path().join("plugin-programs/ffmpeg/1/bin/ffmpeg");
        assert_eq!(inst.bin_path("alpha", &a), program);
        assert_eq!(inst.bin_path("beta", &b), program);
        assert_eq!(std::fs::read(&program).expect("program"), b"static ffmpeg");
        assert!(inst.state("alpha", &a).is_some() && inst.state("beta", &b).is_some());
        assert!(!tmp.path().join("plugin-data").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&program)
                .expect("meta")
                .permissions()
                .mode();
            assert!(mode & 0o111 != 0, "executable");
        }

        // A plugin that comes later finds it in place and downloads nothing.
        inst.install("gamma", &shared_dep("ffmpeg"), &nothing)
            .await
            .expect("gamma");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_newer_entry_is_a_new_folder_and_the_old_versions_are_removed() {
        let base = serve(
            Router::new()
                .route("/one", get(|| async { b"one".to_vec() }))
                .route("/two", get(|| async { b"two".to_vec() })),
        )
        .await;
        let tmp = tempfile::tempdir().expect("tmp");
        let dep = shared_dep("ffmpeg");

        let old = installer(tmp.path(), None).with_shared_programs(vec![shared_entry(
            "1",
            format!("{base}/one"),
            b"one",
        )]);
        old.install("demo", &dep, &nothing).await.expect("v1");
        let programs = tmp.path().join("plugin-programs/ffmpeg");
        assert!(programs.join("1/bin/ffmpeg").is_file());
        // Not versions: a work folder is left alone by the pruning.
        std::fs::create_dir_all(programs.join(".tmp-other")).expect("work folder");

        // FetchBox is updated: its list names version 2, so version 1 is not in place any more.
        let new = installer(tmp.path(), None).with_shared_programs(vec![shared_entry(
            "2",
            format!("{base}/two"),
            b"two",
        )]);
        assert!(new.state("demo", &dep).is_none());
        new.install("demo", &dep, &nothing).await.expect("v2");
        assert_eq!(
            std::fs::read(new.bin_path("demo", &dep)).expect("read"),
            b"two"
        );
        assert!(!programs.join("1").exists(), "the old version is gone");
        assert!(programs.join("2").is_dir());
        assert!(programs.join(".tmp-other").is_dir());
    }

    #[tokio::test]
    async fn a_program_that_was_installed_before_is_fetched_again_when_the_list_moves_on() {
        let base = serve(Router::new().route("/two", get(|| async { b"two".to_vec() }))).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), None).with_shared_programs(vec![shared_entry(
            "2",
            format!("{base}/two"),
            b"two",
        )]);
        let dep = shared_dep("ffmpeg");

        // Nobody ever downloaded it: an optional program stays an opt-in.
        assert!(!inst.was_installed_before("demo", &dep));

        // An earlier version of the program is in place (a work folder does not count).
        let programs = tmp.path().join("plugin-programs/ffmpeg");
        std::fs::create_dir_all(programs.join(".tmp-x")).expect("work folder");
        std::fs::create_dir_all(programs.join("1")).expect("old version");
        assert!(
            !inst.was_installed_before("demo", &dep),
            "no record, not installed"
        );
        std::fs::write(programs.join("1").join(INFO_FILE), "{}").expect("record");
        assert!(inst.was_installed_before("demo", &dep));
        std::fs::remove_dir_all(programs.join("1")).expect("remove");
        assert!(!inst.was_installed_before("demo", &dep));

        // Or it was in the plugin's own folder, from before the manifest named the shared entry.
        let legacy = tmp.path().join("plugin-data/demo/deps/ffmpeg");
        std::fs::create_dir_all(&legacy).expect("legacy");
        std::fs::write(legacy.join("ffmpeg"), b"old copy").expect("program");
        std::fs::write(legacy.join(INFO_FILE), "{}").expect("record");
        assert!(inst.was_installed_before("demo", &dep));
        assert!(
            !inst.was_installed_before("other", &dep),
            "that is another plugin's folder"
        );

        // Only a shared program has a before.
        assert!(!inst.was_installed_before("demo", &github_dep(Some("SUMS"), "x")));

        // The fetch that follows leaves the shared copy and removes the unused one.
        inst.install("demo", &dep, &nothing)
            .await
            .expect("installed");
        assert!(inst.state("demo", &dep).is_some());
        assert!(!legacy.exists(), "the plugin's own copy is gone");
    }

    #[tokio::test]
    async fn a_shared_program_that_does_not_exist_or_cannot_be_linked_is_refused() {
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), None).with_shared_programs(Vec::new());
        let err = inst
            .install("demo", &shared_dep("ffmpeg"), &nothing)
            .await
            .expect_err("unknown");
        assert!(
            err.to_string().contains("no shared program \"ffmpeg\""),
            "{err}"
        );
        assert!(inst.state("demo", &shared_dep("ffmpeg")).is_none());

        let program = tmp.path().join("built");
        std::fs::write(&program, b"x").expect("program");
        let err = link_local(
            &Layout::new(tmp.path()),
            "demo",
            &shared_dep("ffmpeg"),
            &program,
        )
        .expect_err("shared");
        assert!(err.to_string().contains("shared program"), "{err}");
    }

    #[test]
    fn a_local_build_stands_in_for_the_download() {
        let tmp = tempfile::tempdir().expect("tmp");
        let layout = Layout::new(tmp.path().join("data"));
        let built = tmp.path().join("target-release-tool");
        std::fs::write(&built, b"first build").expect("program");
        // A GitHub dependency that no server will be asked about: it is installed already.
        let dep = Dependency {
            id: "tool".into(),
            label: String::new(),
            optional: false,
            bin: None,
            platforms: BTreeMap::new(),
            github: Some(GithubDependency {
                repo: "me/tool".into(),
                assets: BTreeMap::new(),
                sha256_asset: Some("SHA2-256SUMS".into()),
                tag: None,
                sha256: BTreeMap::new(),
            }),
            shared: None,
        };
        let installer = Installer::new(
            Fetcher::new(UrlPolicy::default()).expect("fetcher"),
            layout.clone(),
        );
        assert!(installer.state("demo", &dep).is_none());

        let info = link_local(&layout, "demo", &dep, &built).expect("linked");
        assert_eq!(info.version.as_deref(), Some("local build"));
        let state = installer.state("demo", &dep).expect("installed");
        assert_eq!(state.host, "local");
        let bin = installer.bin_path("demo", &dep);
        assert_eq!(std::fs::read(&bin).expect("read"), b"first build");

        // A rebuild is seen through the link, and linking again replaces what was there.
        std::fs::write(&built, b"second build").expect("rebuild");
        assert_eq!(std::fs::read(&bin).expect("read"), b"second build");
        link_local(&layout, "demo", &dep, &built).expect("linked again");
        assert!(installer.state("demo", &dep).is_some());

        // A program that is not there is an error and leaves the earlier link alone.
        let err = link_local(&layout, "demo", &dep, &tmp.path().join("missing")).expect_err("gone");
        assert!(err.to_string().contains("no program"), "{err}");
        assert!(installer.state("demo", &dep).is_some());
    }

    #[tokio::test]
    async fn a_raw_program_is_downloaded_verified_and_made_executable() {
        let body = b"\x7fELF program".to_vec();
        let served = body.clone();
        let base =
            serve(Router::new().route("/tool", get(move || async move { served.clone() }))).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), None);
        let dep = pinned("tool", format!("{base}/tool"), &body, ArchiveKind::Raw, 0);
        assert_eq!(inst.state("demo", &dep), None);

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let info = inst
            .install("demo", &dep, &move |p| seen2.lock().expect("lock").push(p))
            .await
            .expect("install");
        assert_eq!(info.sha256, sha(&body));
        assert_eq!(info.version, None);
        assert_eq!(info.host, "127.0.0.1");
        let bin = inst.bin_path("demo", &dep);
        assert_eq!(std::fs::read(&bin).expect("read"), body);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&bin).expect("m").permissions().mode() & 0o777,
                0o755
            );
        }
        assert_eq!(inst.state("demo", &dep), Some(info));
        let seen = seen.lock().expect("lock");
        assert_eq!(seen.first().map(|p| p.downloaded), Some(0));
        let last = seen.last().expect("progress");
        assert_eq!(
            (last.downloaded, last.total),
            (body.len() as u64, Some(body.len() as u64))
        );
        assert!(seen.iter().all(|p| p.dependency == "tool"));
        // Nothing but the dependency is left in the plugin's data.
        let left: Vec<_> = std::fs::read_dir(tmp.path().join("plugin-data/demo/deps"))
            .expect("dir")
            .map(|e| e.expect("e").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["tool"]);
    }

    #[tokio::test]
    async fn a_wrong_checksum_installs_nothing() {
        let body = b"real".to_vec();
        let served = body.clone();
        let base =
            serve(Router::new().route("/tool", get(move || async move { served.clone() }))).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), None);
        let mut dep = pinned(
            "tool",
            format!("{base}/tool"),
            b"something else",
            ArchiveKind::Raw,
            0,
        );
        dep.platforms.get_mut("linux-x86_64").expect("src").size = None;
        let err = inst
            .install("demo", &dep, &nothing)
            .await
            .expect_err("mismatch");
        assert!(err.to_string().contains("does not match"), "{err}");
        assert_eq!(inst.state("demo", &dep), None);
        let deps = tmp.path().join("plugin-data/demo/deps");
        assert_eq!(
            std::fs::read_dir(deps).expect("dir").count(),
            0,
            "no leftovers"
        );
    }

    #[tokio::test]
    async fn a_file_larger_than_declared_is_stopped() {
        let big = vec![1u8; 3 * 1024 * 1024];
        let served = big.clone();
        let base =
            serve(Router::new().route("/tool", get(move || async move { served.clone() }))).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), None);
        let mut dep = pinned("tool", format!("{base}/tool"), &big, ArchiveKind::Raw, 0);
        dep.platforms.get_mut("linux-x86_64").expect("src").size = Some(1000);
        let err = inst
            .install("demo", &dep, &nothing)
            .await
            .expect_err("too big");
        assert!(err.to_string().contains("larger than expected"), "{err}");
        assert_eq!(inst.state("demo", &dep), None);
    }

    #[tokio::test]
    async fn an_archive_is_unpacked_with_strip_and_must_contain_the_program() {
        let archive = targz(&[
            ("ffmpeg-7/bin/ffmpeg", b"ELF-ffmpeg", 0o644),
            ("ffmpeg-7/doc/README", b"hi", 0o644),
        ]);
        let served = archive.clone();
        let base =
            serve(Router::new().route("/f.tar.gz", get(move || async move { served.clone() })))
                .await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), None);
        let mut dep = pinned(
            "ffmpeg",
            format!("{base}/f.tar.gz"),
            &archive,
            ArchiveKind::TarGz,
            1,
        );
        dep.bin = Some("bin/ffmpeg".into());
        inst.install("demo", &dep, &nothing).await.expect("install");
        assert_eq!(
            std::fs::read(inst.bin_path("demo", &dep)).expect("read"),
            b"ELF-ffmpeg"
        );
        assert!(inst.dep_dir("demo", &dep).join("doc/README").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(inst.bin_path("demo", &dep))
                .expect("m")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o111,
                0o111,
                "the program is executable even if the archive said no"
            );
        }

        let mut wrong = dep.clone();
        wrong.id = "other".into();
        wrong.bin = Some("bin/missing".into());
        let err = inst
            .install("demo", &wrong, &nothing)
            .await
            .expect_err("no program");
        assert!(err.to_string().contains("does not contain"), "{err}");
        assert_eq!(inst.state("demo", &wrong), None);
    }

    #[tokio::test]
    async fn a_platform_can_name_its_program_itself() {
        let archive = targz(&[
            ("pkg/bin/ffmpeg-linux-x86_64-v7", b"ELF", 0o644),
            ("pkg/README", b"hi", 0o644),
        ]);
        let served = archive.clone();
        let base =
            serve(Router::new().route("/w.whl", get(move || async move { served.clone() }))).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), None);
        let mut dep = pinned(
            "ffmpeg",
            format!("{base}/w.whl"),
            &archive,
            ArchiveKind::TarGz,
            0,
        );
        dep.bin = Some("elsewhere/ffmpeg".into());
        if let Some(src) = dep.platforms.get_mut("linux-x86_64") {
            src.bin = Some("pkg/bin/ffmpeg-linux-x86_64-v7".into());
        }
        inst.install("demo", &dep, &nothing).await.expect("install");
        assert_eq!(
            inst.bin_path("demo", &dep),
            inst.dep_dir("demo", &dep)
                .join("pkg/bin/ffmpeg-linux-x86_64-v7"),
            "the platform's own path wins over the dependency's"
        );
        assert_eq!(
            std::fs::read(inst.bin_path("demo", &dep)).expect("read"),
            b"ELF"
        );
        assert!(inst.state("demo", &dep).is_some());
    }

    #[tokio::test]
    async fn installing_again_replaces_the_old_version() {
        let version = Arc::new(AtomicU64::new(1));
        let v = version.clone();
        let base = serve(Router::new().route(
            "/tool",
            get(move || {
                let n = v.load(Ordering::SeqCst);
                async move { format!("version {n}").into_bytes() }
            }),
        ))
        .await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), None);
        let first = pinned(
            "tool",
            format!("{base}/tool"),
            b"version 1",
            ArchiveKind::Raw,
            0,
        );
        inst.install("demo", &first, &nothing).await.expect("first");
        version.store(2, Ordering::SeqCst);
        let second = pinned(
            "tool",
            format!("{base}/tool"),
            b"version 2",
            ArchiveKind::Raw,
            0,
        );
        inst.install("demo", &second, &nothing)
            .await
            .expect("second");
        assert_eq!(
            std::fs::read(inst.bin_path("demo", &second)).expect("read"),
            b"version 2"
        );

        // A failed update keeps the working version.
        let broken = pinned(
            "tool",
            format!("{base}/tool"),
            b"version 3",
            ArchiveKind::Raw,
            0,
        );
        inst.install("demo", &broken, &nothing)
            .await
            .expect_err("mismatch");
        assert_eq!(
            std::fs::read(inst.bin_path("demo", &second)).expect("read"),
            b"version 2"
        );
        assert!(inst.state("demo", &second).is_some());
    }

    #[tokio::test]
    async fn a_dependency_without_a_file_for_this_platform_says_so() {
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = Installer::for_platform(
            Fetcher::new(UrlPolicy::default()).expect("fetcher"),
            Layout::new(tmp.path()),
            "macos-aarch64".into(),
        );
        let dep = pinned(
            "tool",
            "https://x.example/t".into(),
            b"x",
            ArchiveKind::Raw,
            0,
        );
        let err = inst
            .install("demo", &dep, &nothing)
            .await
            .expect_err("platform");
        assert!(
            err.to_string().contains("not available for macos-aarch64"),
            "{err}"
        );
    }

    #[test]
    fn only_https_addresses_that_the_policy_allows_are_fetched() {
        let strict = Fetcher::new(UrlPolicy::default()).expect("fetcher");
        assert!(strict.check("https://example.com/x").is_ok());
        assert!(strict.check("http://example.com/x").is_err());
        assert!(strict.check("ftp://example.com/x").is_err());
        assert!(strict.check("https://user:pw@example.com/x").is_err());
        assert!(strict.check("not an address").is_err());
        // Loopback is fine by default and refused when private addresses are blocked.
        assert!(strict.check("https://127.0.0.1/x").is_ok());
        let guarded = Fetcher::new(UrlPolicy {
            block_private: true,
        })
        .expect("fetcher");
        for blocked in [
            "https://127.0.0.1/x",
            "https://10.0.0.5/x",
            "https://169.254.169.254/x",
            "https://[::1]/x",
        ] {
            assert!(guarded.check(blocked).is_err(), "{blocked}");
        }
        assert!(guarded.check("https://example.com/x").is_ok());
    }

    #[test]
    fn checksum_lists_are_read_like_sha256sum_writes_them() {
        let list = format!(
            "{a}  yt-dlp\n{b} *yt-dlp_linux\n{c}  yt-dlp.exe\nnot-a-digest  other\n",
            a = "a".repeat(64),
            b = "B".repeat(64),
            c = "c".repeat(64)
        );
        assert_eq!(checksum_for(&list, "yt-dlp_linux"), Some("b".repeat(64)));
        assert_eq!(checksum_for(&list, "yt-dlp"), Some("a".repeat(64)));
        assert_eq!(checksum_for(&list, "other"), None);
        assert_eq!(checksum_for(&list, "missing"), None);
        assert_eq!(checksum_for("", "x"), None);
    }

    #[test]
    fn assets_are_unpacked_by_their_name() {
        assert_eq!(archive_of("ffmpeg.tar.xz"), ArchiveKind::TarXz);
        assert_eq!(archive_of("FFMPEG.TAR.GZ"), ArchiveKind::TarGz);
        assert_eq!(archive_of("x.tgz"), ArchiveKind::TarGz);
        assert_eq!(archive_of("x.zip"), ArchiveKind::Zip);
        assert_eq!(archive_of("x.gz"), ArchiveKind::Gz);
        assert_eq!(archive_of("yt-dlp_linux"), ArchiveKind::Raw);
        assert_eq!(archive_of("yt-dlp.exe"), ArchiveKind::Raw);
    }

    /// A tiny GitHub: the releases API plus the files of one release.
    async fn github(asset_body: Vec<u8>, sums: String) -> String {
        let state = Arc::new((asset_body, sums));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let (s1, s2, s3) = (state.clone(), state.clone(), base.clone());
        let app = Router::new()
            .route(
                "/repos/o/r/releases/latest",
                get(move || {
                    let (state, base) = (s1.clone(), s3.clone());
                    async move {
                        serde_json::json!({
                            "tag_name": "2026.01.01",
                            "assets": [
                                {"name": "tool_linux", "size": state.0.len(), "browser_download_url": format!("{base}/dl/tool_linux")},
                                {"name": "SHA2-256SUMS", "size": state.1.len(), "browser_download_url": format!("{base}/dl/SUMS")},
                            ]
                        })
                        .to_string()
                    }
                }),
            )
            .route("/dl/tool_linux", get(move || { let s = s2.clone(); async move { s.0.clone() } }))
            .route("/dl/SUMS", get({ let s = state.clone(); move || { let s = s.clone(); async move { s.1.clone() } } }));
        tokio::spawn(async move { axum::serve(listener, app).await });
        base
    }

    fn github_dep(sha_asset: Option<&str>, asset: &str) -> Dependency {
        let mut assets = BTreeMap::new();
        assets.insert("linux-x86_64".to_owned(), asset.to_owned());
        Dependency {
            id: "tool".into(),
            label: String::new(),
            optional: false,
            bin: None,
            platforms: BTreeMap::new(),
            github: Some(GithubDependency {
                repo: "o/r".into(),
                assets,
                sha256_asset: sha_asset.map(str::to_owned),
                tag: None,
                sha256: BTreeMap::new(),
            }),
            shared: None,
        }
    }

    #[tokio::test]
    async fn a_github_dependency_follows_the_latest_release_and_is_verified_by_its_checksum_file() {
        let body = b"tool binary".to_vec();
        let sums = format!("{}  tool_linux\n{}  other\n", sha(&body), "0".repeat(64));
        let api = github(body.clone(), sums).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), Some(&api));
        let dep = github_dep(Some("SHA2-256SUMS"), "tool_linux");
        let info = inst.install("demo", &dep, &nothing).await.expect("install");
        assert_eq!(info.version.as_deref(), Some("2026.01.01"));
        assert_eq!(info.sha256, sha(&body));
        assert_eq!(
            std::fs::read(inst.bin_path("demo", &dep)).expect("read"),
            body
        );
    }

    #[tokio::test]
    async fn a_github_download_that_the_checksum_file_does_not_vouch_for_is_refused() {
        let body = b"tool binary".to_vec();
        // The list names a different digest for the file.
        let sums = format!("{}  tool_linux\n", "1".repeat(64));
        let api = github(body, sums).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), Some(&api));
        let dep = github_dep(Some("SHA2-256SUMS"), "tool_linux");
        let err = inst
            .install("demo", &dep, &nothing)
            .await
            .expect_err("mismatch");
        assert!(err.to_string().contains("does not match"), "{err}");
        assert_eq!(inst.state("demo", &dep), None);
    }

    /// A GitHub with one release, `2026.01.01`, that counts how often its checksum file is asked for.
    async fn github_tagged(asset_body: Vec<u8>, sums: String) -> (String, Arc<AtomicU64>) {
        let sums_hits = Arc::new(AtomicU64::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let release = {
            let (base, len, sums_len) = (base.clone(), asset_body.len(), sums.len());
            move || {
                let doc = serde_json::json!({
                    "tag_name": "2026.01.01",
                    "assets": [
                        {"name": "tool_linux", "size": len, "browser_download_url": format!("{base}/dl/tool_linux")},
                        {"name": "SHA2-256SUMS", "size": sums_len, "browser_download_url": format!("{base}/dl/SUMS")},
                    ]
                });
                async move { doc.to_string() }
            }
        };
        let counted = sums_hits.clone();
        let app = Router::new()
            .route("/repos/o/r/releases/tags/2026.01.01", get(release))
            .route(
                "/dl/tool_linux",
                get(move || {
                    let body = asset_body.clone();
                    async move { body }
                }),
            )
            .route(
                "/dl/SUMS",
                get(move || {
                    counted.fetch_add(1, Ordering::SeqCst);
                    let sums = sums.clone();
                    async move { sums }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await });
        (base, sums_hits)
    }

    fn pinned_github_dep(hash: &str, sha_asset: Option<&str>) -> Dependency {
        let mut dep = github_dep(sha_asset, "tool_linux");
        let github = dep.github.as_mut().expect("github");
        github.tag = Some("2026.01.01".into());
        github.sha256.insert("linux-x86_64".into(), hash.into());
        dep
    }

    #[tokio::test]
    async fn a_hash_pinned_in_the_manifest_replaces_the_checksum_file_of_the_release() {
        let body = b"tool binary".to_vec();
        // The release's own list says something else: whoever replaced the asset could have
        // replaced it too, so it is not asked for at all.
        let (api, sums_hits) =
            github_tagged(body.clone(), format!("{}  tool_linux\n", "0".repeat(64))).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), Some(&api));
        let dep = pinned_github_dep(&sha(&body).to_uppercase(), Some("SHA2-256SUMS"));

        let info = inst.install("demo", &dep, &nothing).await.expect("install");
        assert_eq!(info.sha256, sha(&body));
        assert_eq!(info.version.as_deref(), Some("2026.01.01"));
        assert_eq!(
            sums_hits.load(Ordering::SeqCst),
            0,
            "the checksum file was not fetched"
        );
        assert!(inst.state("demo", &dep).is_some());
    }

    #[tokio::test]
    async fn an_asset_that_matches_the_release_list_but_not_the_pinned_hash_is_refused() {
        let body = b"tool binary".to_vec();
        // A swapped asset with a list that vouches for it: the old kind of check would pass.
        let (api, _) = github_tagged(body.clone(), format!("{}  tool_linux\n", sha(&body))).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), Some(&api));
        let dep = pinned_github_dep(&"1".repeat(64), Some("SHA2-256SUMS"));

        let err = inst
            .install("demo", &dep, &nothing)
            .await
            .expect_err("pinned");
        assert!(err.to_string().contains("does not match"), "{err}");
        assert_eq!(inst.state("demo", &dep), None, "nothing was installed");
    }

    #[tokio::test]
    async fn a_rebuilt_asset_of_the_same_release_is_fetched_again_when_the_manifest_pins_it() {
        let old = b"tool binary".to_vec();
        let (api, _) = github_tagged(old.clone(), String::new()).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), Some(&api));

        // Installed against an older manifest that pinned this build...
        let dep = pinned_github_dep(&sha(&old), None);
        inst.install("demo", &dep, &nothing).await.expect("install");
        assert!(inst.state("demo", &dep).is_some());

        // ...and the manifest of an update keeps the tag but names another build.
        let rebuilt = pinned_github_dep(&sha(b"another build"), None);
        assert!(
            inst.state("demo", &rebuilt).is_none(),
            "the program that is there is not the one the manifest pins"
        );
        // A program linked from a local build still stands in.
        let built = tmp.path().join("built");
        std::fs::write(&built, b"local").expect("program");
        link_local(&Layout::new(tmp.path()), "demo", &rebuilt, &built).expect("linked");
        assert!(inst.state("demo", &rebuilt).is_some());
    }

    #[tokio::test]
    async fn github_problems_are_explained() {
        let body = b"tool binary".to_vec();
        let sums = format!("{}  tool_linux\n", sha(&body));
        let api = github(body, sums).await;
        let tmp = tempfile::tempdir().expect("tmp");
        let inst = installer(tmp.path(), Some(&api));

        let err = inst
            .install("demo", &github_dep(Some("SHA2-256SUMS"), "nope"), &nothing)
            .await
            .expect_err("asset");
        assert!(err.to_string().contains("has no file \"nope\""), "{err}");

        let err = inst
            .install("demo", &github_dep(Some("MISSING"), "tool_linux"), &nothing)
            .await
            .expect_err("sums");
        assert!(err.to_string().contains("no checksum file"), "{err}");

        let err = inst
            .install("demo", &github_dep(None, "tool_linux"), &nothing)
            .await
            .expect_err("none");
        assert!(err.to_string().contains("no checksum file"), "{err}");

        // Another repository: the server answers 404.
        let mut other = github_dep(Some("SHA2-256SUMS"), "tool_linux");
        other.github.as_mut().expect("github").repo = "x/y".into();
        let err = inst
            .install("demo", &other, &nothing)
            .await
            .expect_err("404");
        assert!(err.to_string().contains("404"), "{err}");

        let mut no_platform = github_dep(Some("SHA2-256SUMS"), "tool_linux");
        no_platform.github.as_mut().expect("github").assets.clear();
        let err = inst
            .install("demo", &no_platform, &nothing)
            .await
            .expect_err("platform");
        assert!(
            err.to_string().contains("no release file for linux-x86_64"),
            "{err}"
        );
    }
}
