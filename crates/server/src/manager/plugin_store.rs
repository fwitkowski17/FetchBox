//! The plugin store in the manager (SPEC 6.15): the plugins that ship inside the binary, the
//! remote catalog, and installing from either.
//!
//! A bundled plugin works offline and needs no ZIP: its files are embedded at compile time and
//! written to a staging folder like an uploaded package, so it goes through the same review. A
//! plugin from the remote catalog is downloaded, its SHA-256 compared with the catalog's before
//! anything is unpacked, and its manifest must be the plugin the catalog named.

use std::path::Path;
use std::sync::OnceLock;

use fetchbox_core::plugin::{
    self, platform_key, PluginSource, StagedPlugin, StoreEntry, StoreList, MAX_PLUGIN_ZIP,
};
use fetchbox_core::version::is_newer;
use fetchbox_core::{Error, Result};
use fetchbox_plugins::install;
use rust_embed::RustEmbed;

use super::Manager;

/// The plugins that ship with FetchBox: one folder each, with a `plugin.json`.
#[derive(RustEmbed)]
#[folder = "../../plugins/official"]
struct Bundled;

/// Where the catalog of third-party plugins is, unless `FETCHBOX_PLUGIN_INDEX_URL` says otherwise.
pub const DEFAULT_INDEX_URL: &str =
    "https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/plugins/index.json";

/// The store entries of the bundled plugins (read once).
fn bundled_entries() -> &'static [StoreEntry] {
    static ENTRIES: OnceLock<Vec<StoreEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let mut out = Vec::new();
        for path in Bundled::iter() {
            let Some(id) = path.strip_suffix("/plugin.json") else {
                continue;
            };
            let Some(file) = Bundled::get(&path) else { continue };
            match install::parse_manifest(&file.data) {
                Ok(m) if m.id == id => out.push(StoreEntry {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    version: m.version.clone(),
                    description: m.description.clone(),
                    author: m.author.clone(),
                    homepage: m.homepage.clone(),
                    license: m.license.clone(),
                    icon: m.ui.icon.clone(),
                    tags: Vec::new(),
                    min_fetchbox: m.min_fetchbox.clone(),
                    platforms: m.platforms.clone(),
                    bundled: true,
                    download: None,
                    installed: None,
                    update_available: false,
                    unsupported: false,
                }),
                Ok(m) => tracing::error!(folder = id, plugin = %m.id, "a bundled plugin's folder is not its id"),
                Err(e) => tracing::error!(folder = id, error = %e, "a bundled plugin is invalid"),
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    })
}

/// Write the files of a bundled plugin into `dest`.
fn write_bundled(id: &str, dest: &Path) -> std::io::Result<usize> {
    let prefix = format!("{id}/");
    let mut written = 0;
    for path in Bundled::iter() {
        let Some(rel) = path.strip_prefix(&prefix) else {
            continue;
        };
        let Some(file) = Bundled::get(&path) else {
            continue;
        };
        let target = dest.join(rel);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&target, &file.data)?;
        #[cfg(unix)]
        if rel.starts_with("bin/") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
        }
        written += 1;
    }
    Ok(written)
}

/// Why a plugin cannot run here, for an entry that has not been downloaded yet.
fn entry_unsupported(e: &StoreEntry) -> bool {
    (!e.platforms.is_empty() && !e.platforms.contains(&platform_key()))
        || e.min_fetchbox
            .as_deref()
            .is_some_and(|min| is_newer(min, env!("CARGO_PKG_VERSION")))
}

impl Manager {
    /// The newest version of a plugin that the store knows (bundled, or in the catalog fetched so
    /// far), if it is newer than `installed`. Never fetches: it is used to build every view.
    pub(super) fn store_update_for(&self, id: &str, installed: &str) -> Option<String> {
        let remote = self
            .plugins
            .host
            .get()
            .map(|h| h.catalog.cached().entries)
            .unwrap_or_default();
        bundled_entries()
            .iter()
            .chain(remote.iter())
            .filter(|e| e.id == id && !entry_unsupported(e))
            .map(|e| e.version.clone())
            .filter(|v| is_newer(v, installed))
            .reduce(|a, b| if is_newer(&b, &a) { b } else { a })
    }

    /// Everything that can be installed: what ships with FetchBox and the remote catalog, one entry
    /// per plugin (the newer version wins; at equal versions the bundled one, which needs no
    /// download), marked with what is installed. `force` fetches the catalog again.
    pub(super) async fn plugins_store(&self, force: bool) -> Result<StoreList> {
        let host = self.plugin_host()?;
        let snapshot = host.catalog.get(force).await;
        let mut by_id: std::collections::BTreeMap<String, StoreEntry> = bundled_entries()
            .iter()
            .map(|e| (e.id.clone(), e.clone()))
            .collect();
        for remote in snapshot.entries {
            match by_id.get(&remote.id) {
                Some(have) if !is_newer(&remote.version, &have.version) => {}
                _ => {
                    by_id.insert(remote.id.clone(), remote);
                }
            }
        }
        let installed: std::collections::HashMap<String, String> = self
            .plugin_entries()
            .iter()
            .filter_map(|(id, e)| e.manifest.as_ref().map(|m| (id.clone(), m.version.clone())))
            .collect();
        let mut entries: Vec<StoreEntry> = by_id
            .into_values()
            .map(|mut e| {
                e.unsupported = entry_unsupported(&e);
                e.installed = installed.get(&e.id).cloned();
                e.update_available = e
                    .installed
                    .as_deref()
                    .is_some_and(|have| is_newer(&e.version, have));
                e
            })
            .collect();
        entries.sort_by_key(|e| e.name.to_lowercase());
        Ok(StoreList {
            entries,
            remote: host.catalog.enabled(),
            fetched_at: snapshot.fetched_at,
            error: snapshot.error,
        })
    }

    /// Unpack a plugin of the store into a staging folder, ready for the user's review.
    pub(super) async fn plugins_stage_store(&self, id: String) -> Result<StagedPlugin> {
        let host = self.plugin_host()?;
        if !plugin::is_valid_id(&id) {
            return Err(Error::Missing(format!("the store has no plugin \"{id}\"")));
        }
        let mut list = self.plugins_store(false).await?;
        if !list.entries.iter().any(|e| e.id == id) && list.remote {
            list = self.plugins_store(true).await?;
        }
        let entry = list
            .entries
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| Error::Missing(format!("the store has no plugin \"{id}\"")))?;
        if entry.unsupported {
            return Err(Error::InvalidInput(format!(
                "{} cannot be used here (it needs another platform or a newer FetchBox)",
                entry.name
            )));
        }
        let layout = host.layout.clone();
        let (staged, source, sha) = match &entry.download {
            None => {
                let id2 = id.clone();
                let staged = tokio::task::spawn_blocking(
                    move || -> fetchbox_plugins::Result<install::Staged> {
                        let (token, pkg) = install::new_staging(&layout)?;
                        if let Err(e) = write_bundled(&id2, &pkg) {
                            install::cancel(&layout, &token);
                            return Err(e.into());
                        }
                        install::finish_staged(&layout, &token)
                    },
                )
                .await
                .map_err(|e| Error::Internal(e.to_string()))?
                .map_err(Error::from)?;
                (staged, PluginSource::Bundled, None)
            }
            Some(download) => {
                let path = layout.upload_path().map_err(Error::from)?;
                let max = download
                    .size
                    .map_or(MAX_PLUGIN_ZIP as u64, |s| s + s / 10 + 4096);
                let got = host
                    .fetcher
                    .download(
                        &download.url,
                        &path,
                        max.min(MAX_PLUGIN_ZIP as u64),
                        &|_, _| {},
                    )
                    .await;
                let got = match got {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = std::fs::remove_file(&path);
                        return Err(e.into());
                    }
                };
                if got != download.sha256 {
                    let _ = std::fs::remove_file(&path);
                    return Err(Error::Engine(format!(
                        "the download of {} does not match the checksum in the catalog",
                        entry.name
                    )));
                }
                let layout2 = layout.clone();
                let staged = tokio::task::spawn_blocking(move || {
                    let r = install::stage_zip(&layout2, &path);
                    let _ = std::fs::remove_file(&path);
                    r
                })
                .await
                .map_err(|e| Error::Internal(e.to_string()))?
                .map_err(Error::from)?;
                (staged, PluginSource::Store, Some(download.sha256.clone()))
            }
        };
        if staged.manifest.id != id {
            install::cancel(&host.layout, &staged.token);
            return Err(Error::InvalidInput(format!(
                "the package is the plugin \"{}\", not \"{id}\" as the catalog said",
                staged.manifest.id
            )));
        }
        self.finish_stage(staged, source, sha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundled manifest that does not validate is only logged at run time, so a typo would ship
    /// silently: this is where it is caught.
    #[test]
    fn every_bundled_plugin_is_valid() {
        let mut broken = Vec::new();
        let mut ids = Vec::new();
        for path in Bundled::iter() {
            let Some(folder) = path.strip_suffix("/plugin.json") else {
                continue;
            };
            let file = Bundled::get(&path).expect("listed file");
            match install::parse_manifest(&file.data) {
                Ok(m) if m.id == folder => ids.push(m.id),
                Ok(m) => broken.push(format!("{folder}: the id is {}", m.id)),
                Err(e) => broken.push(format!("{folder}: {e}")),
            }
        }
        assert!(broken.is_empty(), "{broken:?}");
        ids.sort();
        assert_eq!(ids, ["webhook-notify", "yt-dlp"]);
    }

    #[test]
    fn the_store_lists_what_ships_with_the_program() {
        let names: Vec<_> = bundled_entries().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(names, ["webhook-notify", "yt-dlp"]);
    }
}
