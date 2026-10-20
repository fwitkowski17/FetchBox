//! Resolvers of plugins in the manager (SPEC 6.15): a plugin that understands some addresses (a
//! video page, a gallery) says what is behind one, and the add dialog shows it like a directory
//! listing.
//!
//! Two kinds of resolver:
//! - one with a `run` command: the plugin's program makes the file (it is the only way to get a
//!   video that is split into streams), so each entry of the listing is added with `via`;
//! - one without: it only finds addresses, and FetchBox's engine downloads them like any other.

use std::collections::BTreeMap;
use std::time::Duration;

use fetchbox_core::category::detect as detect_category;
use fetchbox_core::filename::sanitize;
use fetchbox_core::plugin::{glob_match, Choice, Manifest, PluginState, Resolver, SettingValue};
use fetchbox_core::{Category, Error, ListingEntry, Result, SourceKind, UrlInfo, UrlKind, Via};
use fetchbox_plugins::expand;
use fetchbox_plugins::mapping::{self, Mapped};
use fetchbox_plugins::oneshot::Spec;

use super::plugins::{effective, Host};
use super::Manager;

/// How long a plugin's long-lived program may take to understand an address.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(60);

/// What a download added `via` a plugin needs to remember.
#[derive(Debug, Clone)]
pub(super) struct ViaPlan {
    /// `<plugin>/<resolver>`.
    pub runner: String,
    /// The preset.
    pub choice: String,
    /// The extension the file will get, when the preset knows.
    pub ext: Option<String>,
    pub category: Option<Category>,
}

/// The patterns of a resolver: its `match` globs, with `${setting:key}` replaced by the lines and
/// comma-separated items of that setting (how a user adds sites).
fn patterns(r: &Resolver, settings: &BTreeMap<String, SettingValue>) -> Vec<String> {
    let mut out = Vec::new();
    for g in &r.matches {
        match g
            .strip_prefix("${setting:")
            .and_then(|x| x.strip_suffix('}'))
        {
            Some(key) => {
                let text = expand::setting_text(settings.get(key));
                out.extend(
                    text.split(['\n', '\r', ','])
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                        .map(str::to_owned),
                );
            }
            None => out.push(g.clone()),
        }
    }
    out
}

fn with_ext(name: &str, ext: Option<&str>) -> String {
    match ext {
        Some(e)
            if !name
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", e.to_ascii_lowercase())) =>
        {
            format!("{name}.{e}")
        }
        _ => name.to_owned(),
    }
}

impl Manager {
    /// The plugins that are on and usable, with their manifests.
    fn usable_plugins(&self) -> Vec<(String, Manifest)> {
        self.plugin_entries()
            .iter()
            .filter(|(_, e)| matches!(e.state, PluginState::Ready | PluginState::Running))
            .filter_map(|(id, e)| e.manifest.clone().map(|m| (id.clone(), m)))
            .collect()
    }

    /// Can the preset run now (the dependencies it needs are installed)? `Err` names what is missing.
    fn choice_ready(
        &self,
        host: &Host,
        plugin: &str,
        m: &Manifest,
        c: &Choice,
    ) -> std::result::Result<(), String> {
        for need in &c.requires {
            let installed = m
                .dependencies
                .iter()
                .find(|d| d.id == *need)
                .is_some_and(|d| host.installer.state(plugin, d).is_some());
            if !installed {
                let label =
                    m.dependencies
                        .iter()
                        .find(|d| d.id == *need)
                        .map_or(need.as_str(), |d| {
                            if d.label.is_empty() {
                                &d.id
                            } else {
                                &d.label
                            }
                        });
                return Err(label.to_owned());
            }
        }
        Ok(())
    }

    /// If a plugin understands the address, ask it and describe what it found as a listing.
    /// `Ok(None)`: no plugin wants this address, the engine looks at it as usual.
    pub(super) async fn plugins_resolve(&self, url: &str) -> Result<Option<UrlInfo>> {
        let Ok(host) = self.plugin_host() else {
            return Ok(None);
        };
        let mut found = None;
        for (id, m) in self.usable_plugins() {
            let settings = effective(&m, &self.plugin_stored_settings(&id));
            for r in &m.resolvers {
                if patterns(r, &settings).iter().any(|p| glob_match(p, url)) {
                    found = Some((id.clone(), m.clone(), r.clone(), settings.clone()));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let Some((id, m, resolver, settings)) = found else {
            return Ok(None);
        };
        tracing::info!(target: "activity", plugin = %id, resolver = %resolver.id,
            from = %crate::logfmt::host(url), "plugin resolves an address");
        let mapped = self.run_resolver(host, &id, &resolver, url).await?;
        let info = self.describe(host, &id, &m, &resolver, &settings, url, mapped)?;
        Ok(Some(info))
    }

    async fn run_resolver(&self, host: &Host, id: &str, r: &Resolver, url: &str) -> Result<Mapped> {
        let fail = |e: fetchbox_plugins::Error| Error::Engine(format!("{id}: {e}"));
        let value: serde_json::Value = if r.process {
            let proc = self
                .plugin_entries()
                .get(id)
                .and_then(|e| e.process.clone())
                .ok_or_else(|| Error::InvalidState(format!("{id}: its program is not running")))?;
            proc.request(
                "resolve",
                serde_json::json!({"url": url, "resolver": r.id}),
                RESOLVE_TIMEOUT,
            )
            .await
            .map_err(fail)?
        } else {
            let probe = r
                .probe
                .as_ref()
                .ok_or_else(|| Error::Internal("a resolver without a probe".into()))?;
            let env = self.run_env(id)?;
            let mut ctx = env.context();
            ctx.url = Some(url);
            let program = expand::resolve_program(&probe.command, &ctx).map_err(fail)?;
            let args = expand::expand_args(&probe.args, &ctx).map_err(fail)?;
            let spec = Spec::new(
                program,
                args,
                env.child_env(BTreeMap::new()),
                Duration::from_secs(u64::from(probe.timeout_s.clamp(1, 120))),
            );
            let out = host.runner.run(id, spec).await.map_err(fail)?;
            serde_json::from_slice(&out.stdout).map_err(|_| {
                Error::Engine(format!(
                    "{id}: the program did not print JSON about that address"
                ))
            })?
        };
        let map = r
            .result
            .as_ref()
            .ok_or_else(|| Error::Internal("a resolver without a result mapping".into()))?;
        mapping::map_result(&value, map).map_err(fail)
    }

    #[allow(clippy::too_many_arguments)]
    fn describe(
        &self,
        host: &Host,
        id: &str,
        m: &Manifest,
        r: &Resolver,
        settings: &BTreeMap<String, SettingValue>,
        url: &str,
        mapped: Mapped,
    ) -> Result<UrlInfo> {
        let ready: Vec<&Choice> = r
            .choices
            .iter()
            .filter(|c| self.choice_ready(host, id, m, c).is_ok())
            .collect();
        let via = |choice: Option<&Choice>| {
            choice.map(|c| Via {
                plugin: id.to_owned(),
                resolver: r.id.clone(),
                reference: Some(c.id.clone()),
            })
        };
        let category_of = |c: Option<&Choice>, name: &str| {
            c.and_then(|c| c.category).unwrap_or_else(|| {
                detect_category(&with_ext(name, c.and_then(|c| c.ext.as_deref())), None)
            })
        };
        // A resolver that makes the file needs a preset that can run; say what is missing if none can.
        let need_choice = || -> Result<()> {
            if r.run.is_some() && ready.is_empty() {
                let missing: Vec<String> = r
                    .choices
                    .iter()
                    .filter_map(|c| self.choice_ready(host, id, m, c).err())
                    .collect();
                return Err(Error::InvalidState(format!(
                    "{}: install {} first (Extensions, then the plugin's card)",
                    m.name,
                    missing
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "its dependencies".into())
                )));
            }
            Ok(())
        };
        match mapped {
            Mapped::Single { name, size } => {
                need_choice()?;
                let entries = ready
                    .iter()
                    .map(|c| ListingEntry {
                        name: format!("{name} - {}", c.label),
                        url: url.to_owned(),
                        size,
                        is_dir: false,
                        category: category_of(Some(c), &name),
                        via: via(Some(c)),
                    })
                    .collect();
                Ok(UrlInfo {
                    kind: UrlKind::Listing,
                    filename: sanitize(&name),
                    size: None,
                    resumable: false,
                    category: Category::Other,
                    entries,
                    truncated: false,
                })
            }
            Mapped::List {
                items, truncated, ..
            } => {
                need_choice()?;
                let wanted = r
                    .result
                    .as_ref()
                    .and_then(|x| x.default_choice_setting.as_ref())
                    .map(|k| expand::setting_text(settings.get(k)));
                let default = wanted
                    .and_then(|w| ready.iter().copied().find(|c| c.id == w))
                    .or_else(|| ready.first().copied());
                let entries = items
                    .into_iter()
                    .filter(|i| r.run.is_some() || SourceKind::for_url(&i.url).is_some())
                    .map(|i| {
                        let choice = if r.run.is_some() { default } else { None };
                        ListingEntry {
                            category: category_of(choice, &i.name),
                            via: via(choice),
                            name: i.name,
                            url: i.url,
                            size: i.size,
                            is_dir: false,
                        }
                    })
                    .collect();
                Ok(UrlInfo {
                    kind: UrlKind::Listing,
                    filename: String::new(),
                    size: None,
                    resumable: false,
                    category: Category::Other,
                    entries,
                    truncated,
                })
            }
        }
    }

    /// Check a `via` of an add request and say what to remember about the download.
    pub(super) fn plugins_check_via(&self, via: &Via) -> Result<ViaPlan> {
        let host = self.plugin_host()?;
        let (m, state) = {
            let entries = self.plugin_entries();
            let e = entries
                .get(&via.plugin)
                .ok_or_else(|| Error::InvalidInput(format!("no plugin \"{}\"", via.plugin)))?;
            (e.manifest.clone(), e.state)
        };
        let m = m.ok_or_else(|| Error::InvalidInput(format!("no plugin \"{}\"", via.plugin)))?;
        if !matches!(state, PluginState::Ready | PluginState::Running) {
            return Err(Error::InvalidState(format!("{} is not running", m.name)));
        }
        let r = m
            .resolvers
            .iter()
            .find(|r| r.id == via.resolver && r.run.is_some())
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "{} has no resolver \"{}\" that makes files",
                    m.name, via.resolver
                ))
            })?;
        let choice = match &via.reference {
            Some(want) => r.choices.iter().find(|c| c.id == *want).ok_or_else(|| {
                Error::InvalidInput(format!("{} has no preset \"{want}\"", m.name))
            })?,
            None => r
                .choices
                .iter()
                .find(|c| self.choice_ready(host, &via.plugin, &m, c).is_ok())
                .ok_or_else(|| Error::InvalidState(format!("{}: no preset can run yet", m.name)))?,
        };
        if let Err(missing) = self.choice_ready(host, &via.plugin, &m, choice) {
            return Err(Error::InvalidState(format!(
                "\"{}\" needs {missing}: install it first",
                choice.label
            )));
        }
        Ok(ViaPlan {
            runner: format!("{}/{}", via.plugin, r.id),
            choice: choice.id.clone(),
            ext: choice.ext.clone(),
            category: choice.category,
        })
    }
}

/// The name a download made by a plugin gets before its real extension is known.
pub(super) fn planned_name(name: &str, plan: &ViaPlan) -> String {
    sanitize(&with_ext(name, plan.ext.as_deref()))
}

impl Manager {
    /// What the add path needs to know about an address that a plugin will handle: not the answer of
    /// a server, only a name for the file (the engine never sees the address).
    pub(super) fn plugin_page_info(&self, url: &str) -> Result<fetchbox_http_engine::RemoteInfo> {
        let parsed = fetchbox_http_engine::validate_url(url)
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        self.url_policy
            .check(&parsed)
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        let last = parsed
            .path_segments()
            .and_then(|mut s| s.next_back().map(str::to_owned))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "download".to_owned());
        Ok(fetchbox_http_engine::RemoteInfo {
            size: None,
            // A program that continues where it stopped is the plugin's business (yt-dlp does).
            resumable: true,
            etag: None,
            filename: sanitize(&last),
            content_type: None,
        })
    }

    /// Start a download that a plugin's program makes. Not available yet.
    pub(super) async fn plugins_start_download(&self, _id: fetchbox_core::Id) -> Result<bool> {
        Err(Error::Engine(
            "downloads made by a plugin cannot be started by this build yet".into(),
        ))
    }
}
