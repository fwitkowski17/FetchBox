//! A plugin's dependencies: programs the manifest names and FetchBox downloads for it (SPEC 6.15).
//!
//! A dependency is fetched into `<data>/plugin-data/<plugin>/deps/<id>/` and used from there. The
//! download is verified against a SHA-256 the manifest states (or the release's checksum file, for a
//! GitHub dependency) before anything is unpacked, unpacked beside the final place and swapped in
//! with a rename, so a failed or interrupted install leaves the old version (or nothing) behind.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fetchbox_core::plugin::{ArchiveKind, Dependency, GithubDependency};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::archive::{self, Limits};
use crate::fetch::Fetcher;
use crate::install::{hex, Layout};
use crate::{Error, Result};

/// Name of the file that records what was installed, inside the dependency's folder.
const INFO_FILE: &str = ".fetchbox-dependency.json";
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
            gate: Semaphore::new(1),
        }
    }

    fn deps_dir(&self, plugin: &str) -> PathBuf {
        self.layout.data_dir_of(plugin).join("deps")
    }

    /// The folder of a dependency.
    pub fn dep_dir(&self, plugin: &str, dep: &str) -> PathBuf {
        self.deps_dir(plugin).join(dep)
    }

    /// The dependency's program.
    pub fn bin_path(&self, plugin: &str, dep: &Dependency) -> PathBuf {
        self.dep_dir(plugin, &dep.id).join(self.program_name(dep))
    }

    /// Where the program is inside the download: this platform's own `bin`, else the dependency's,
    /// else its id.
    fn program_name<'a>(&self, dep: &'a Dependency) -> &'a str {
        dep.platforms
            .get(&self.platform)
            .and_then(|s| s.bin.as_deref())
            .or(dep.bin.as_deref())
            .unwrap_or(&dep.id)
    }

    /// What is installed, if the dependency is in place (its record is there and so is its program).
    pub fn state(&self, plugin: &str, dep: &Dependency) -> Option<DepInfo> {
        if !self.bin_path(plugin, dep).is_file() {
            return None;
        }
        let raw = std::fs::read(self.dep_dir(plugin, &dep.id).join(INFO_FILE)).ok()?;
        serde_json::from_slice(&raw).ok()
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
        let resolved = self.resolve(dep).await?;
        let deps = self.deps_dir(plugin);
        tokio::fs::create_dir_all(&deps).await?;
        let work = deps.join(format!(".tmp-{}", hex(&random()?)));
        tokio::fs::create_dir_all(&work).await?;
        let result = self
            .install_in(plugin, dep, &resolved, &work, progress)
            .await;
        let _ = tokio::fs::remove_dir_all(&work).await;
        let info = result?;
        tracing::info!(
            target: "activity",
            plugin,
            dependency = %dep.id,
            version = info.version.as_deref().unwrap_or("-"),
            host = %info.host,
            "plugin dependency installed"
        );
        Ok(info)
    }

    async fn install_in(
        &self,
        plugin: &str,
        dep: &Dependency,
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
        let bin = self.program_name(dep).to_owned();
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
        let target = self.dep_dir(plugin, &dep.id);
        let old = work.join("previous");
        let had_old = tokio::fs::try_exists(&target).await?;
        if had_old {
            tokio::fs::rename(&target, &old).await?;
        }
        if let Err(e) = tokio::fs::rename(&out, &target).await {
            if had_old {
                let _ = tokio::fs::rename(&old, &target).await;
            }
            return Err(Error::Io(e));
        }
        Ok(info)
    }
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
        assert!(inst.dep_dir("demo", "ffmpeg").join("doc/README").is_file());
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
            inst.dep_dir("demo", "ffmpeg")
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
            }),
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
