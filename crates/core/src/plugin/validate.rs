//! Checks a manifest before anything of it is used. The first problem found is returned, worded for
//! the plugin's author.

use std::collections::HashSet;

use super::manifest::{
    Action, ActionKind, Arg, Command, Dialog, Field, FieldKind, HookAction, Manifest, Resolver,
    UiKind, Webhook, WebhookBody,
};
use super::template::{self, Token};
use super::text::{is_safe_relative, is_valid_id, is_valid_key, PLATFORMS};
use super::API_VERSION;
use crate::version::parse_version;

/// The events a hook may name. `download.*`, `torrent.*` and `*` match groups of them.
pub const EVENT_NAMES: [&str; 17] = [
    "download.added",
    "download.started",
    "download.paused",
    "download.resumed",
    "download.completed",
    "download.failed",
    "download.cancelled",
    "download.removed",
    "download.progress",
    "torrent.added",
    "torrent.started",
    "torrent.paused",
    "torrent.completed",
    "torrent.removed",
    "torrent.progress",
    "settings.changed",
    "plugin.settings_changed",
];

const MAX_ITEMS: usize = 64;
const API_PARTS: [&str; 2] = ["downloads.read", "downloads.write"];
const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

/// What the templates of a command may refer to.
#[derive(Clone, Copy, Default)]
struct Allow {
    url: bool,
    choice: bool,
    out_dir: bool,
}

struct Ctx<'a> {
    settings: HashSet<&'a str>,
    deps: HashSet<&'a str>,
    actions: HashSet<&'a str>,
    dialogs: HashSet<&'a str>,
    has_process: bool,
}

type Check = Result<(), String>;

fn text(what: &str, s: &str, max: usize) -> Check {
    if s.chars().count() > max {
        return Err(format!("{what} is longer than {max} characters"));
    }
    if s.chars().any(|c| c.is_control() && c != '\n') {
        return Err(format!("{what} has a control character"));
    }
    Ok(())
}

fn is_https(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|r| !r.is_empty())
}

fn is_http(url: &str) -> bool {
    is_https(url) || url.strip_prefix("http://").is_some_and(|r| !r.is_empty())
}

fn unique<'a>(what: &str, ids: impl Iterator<Item = &'a str>) -> Check {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(format!("{what} \"{id}\" appears twice"));
        }
    }
    Ok(())
}

fn check_template(s: &str, allow: Allow, ctx: &Ctx<'_>) -> Check {
    for t in template::parse(s)? {
        match t {
            Token::Setting(k) if !ctx.settings.contains(k.as_str()) => {
                return Err(format!(
                    "\"{s}\" uses the setting \"{k}\", which is not declared"
                ));
            }
            Token::Dep(d) | Token::DepDir(d) if !ctx.deps.contains(d.as_str()) => {
                return Err(format!(
                    "\"{s}\" uses the dependency \"{d}\", which is not declared"
                ));
            }
            Token::Url if !allow.url => {
                return Err(format!("`${{url}}` cannot be used in \"{s}\" here"));
            }
            Token::ChoiceArgs if !allow.choice => {
                return Err(format!("`${{choice.args}}` cannot be used in \"{s}\" here"));
            }
            Token::OutDir if !allow.out_dir => {
                return Err(format!("`${{out_dir}}` cannot be used in \"{s}\" here"));
            }
            _ => {}
        }
    }
    Ok(())
}

/// `${dep:id}`, `${plugin_dir}/path` or a relative path of the plugin: never a system program.
fn check_program(command: &str, ctx: &Ctx<'_>) -> Check {
    let tokens = template::parse(command)?;
    let ok = match tokens.as_slice() {
        [Token::Dep(d)] => ctx.deps.contains(d.as_str()),
        [Token::PluginDir, Token::Literal(l)] => l.strip_prefix('/').is_some_and(is_safe_relative),
        [Token::Literal(l)] => is_safe_relative(l),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "the command \"{command}\" must be `${{dep:<id>}}` of a declared dependency or a path inside the plugin (`bin/tool` or `${{plugin_dir}}/bin/tool`)"
        ))
    }
}

fn check_args(args: &[Arg], allow: Allow, ctx: &Ctx<'_>) -> Check {
    if args.len() > 128 {
        return Err("a command has more than 128 arguments".into());
    }
    for a in args {
        match a {
            Arg::Plain(s) => check_template(s, allow, ctx)?,
            Arg::When { when, args } => {
                match template::parse_condition(when)? {
                    template::Condition::Setting(k) if !ctx.settings.contains(k.as_str()) => {
                        return Err(format!(
                            "the condition \"{when}\" names an undeclared setting"
                        ));
                    }
                    template::Condition::Dep(d) if !ctx.deps.contains(d.as_str()) => {
                        return Err(format!(
                            "the condition \"{when}\" names an undeclared dependency"
                        ));
                    }
                    _ => {}
                }
                for s in args {
                    check_template(s, allow, ctx)?;
                }
            }
        }
    }
    Ok(())
}

fn check_command(c: &Command, allow: Allow, ctx: &Ctx<'_>) -> Check {
    check_program(&c.command, ctx)?;
    check_args(&c.args, allow, ctx)?;
    if !(1..=120).contains(&c.timeout_s) {
        return Err("timeout_s must be between 1 and 120".into());
    }
    Ok(())
}

fn check_webhook(w: &Webhook, ctx: &Ctx<'_>) -> Check {
    let url_ok = is_http(&w.url)
        || w.url
            .strip_prefix("${setting:")
            .and_then(|r| r.strip_suffix('}'))
            .is_some_and(|k| ctx.settings.contains(k));
    if !url_ok {
        return Err(format!(
            "the webhook address \"{}\" must be an http(s) address or a `${{setting:<key>}}`",
            w.url
        ));
    }
    if let Some(m) = &w.method {
        if !METHODS.contains(&m.as_str()) {
            return Err(format!(
                "the webhook method \"{m}\" is not one of {METHODS:?}"
            ));
        }
    }
    if w.body == WebhookBody::Template && w.template.is_none() {
        return Err("a webhook with body \"template\" needs a `template`".into());
    }
    for v in w.headers.values() {
        check_template(v, Allow::default(), ctx)?;
    }
    if let Some(t) = &w.template {
        text("the webhook template", t, 8000)?;
    }
    Ok(())
}

fn check_fields(what: &str, fields: &[Field], dialog: bool) -> Check {
    if fields.len() > MAX_ITEMS {
        return Err(format!("{what} has more than {MAX_ITEMS} fields"));
    }
    unique(
        &format!("{what} field"),
        fields.iter().map(|f| f.key.as_str()),
    )?;
    let keys: HashSet<&str> = fields.iter().map(|f| f.key.as_str()).collect();
    for f in fields {
        if !is_valid_key(&f.key) {
            return Err(format!(
                "{what}: \"{}\" is not a valid key (lowercase letters, digits, `_`, `-`)",
                f.key
            ));
        }
        if f.kind == FieldKind::Output && !dialog {
            return Err(format!(
                "{what}: \"{}\" is an `output` field, which only dialogs have",
                f.key
            ));
        }
        if f.kind == FieldKind::Select && f.options.is_empty() {
            return Err(format!("{what}: the select \"{}\" has no options", f.key));
        }
        if f.kind == FieldKind::Secret && f.default.is_some() {
            return Err(format!(
                "{what}: the secret \"{}\" cannot have a default",
                f.key
            ));
        }
        if let (Some(lo), Some(hi)) = (f.min, f.max) {
            if lo > hi {
                return Err(format!("{what}: \"{}\" has min above max", f.key));
            }
        }
        if let Some(s) = &f.show_if {
            if !keys.contains(s.key.as_str()) {
                return Err(format!(
                    "{what}: \"{}\" is shown depending on \"{}\", which is not a field",
                    f.key, s.key
                ));
            }
        }
        text("a label", &f.label, 200)?;
        text("a help text", &f.help, 2000)?;
        text("a placeholder", &f.placeholder, 200)?;
        for o in &f.options {
            text("an option label", &o.label, 200)?;
        }
    }
    Ok(())
}

fn check_resolver(r: &Resolver, ctx: &Ctx<'_>) -> Check {
    let name = format!("resolver \"{}\"", r.id);
    if !is_valid_key(&r.id) {
        return Err(format!("{name}: not a valid id"));
    }
    if r.matches.is_empty() {
        return Err(format!("{name} has no `match` patterns"));
    }
    for g in &r.matches {
        let setting = g
            .strip_prefix("${setting:")
            .and_then(|x| x.strip_suffix('}'));
        match setting {
            Some(k) if ctx.settings.contains(k) => {}
            Some(k) => {
                return Err(format!(
                    "{name}: `match` uses the undeclared setting \"{k}\""
                ))
            }
            None if g.is_empty() || g.contains("${") => {
                return Err(format!("{name}: \"{g}\" is not a usable pattern"));
            }
            None => {}
        }
    }
    if r.process {
        if !ctx.has_process {
            return Err(format!(
                "{name} asks the plugin's process, but the plugin has none"
            ));
        }
    } else {
        let Some(probe) = &r.probe else {
            return Err(format!("{name} needs a `probe` (or `process: true`)"));
        };
        check_command(
            probe,
            Allow {
                url: true,
                ..Allow::default()
            },
            ctx,
        )?;
        let Some(result) = &r.result else {
            return Err(format!("{name} has a `probe` but no `result` mapping"));
        };
        if result.items.is_some() && result.item.is_none() {
            return Err(format!("{name}: `result.items` needs `result.item`"));
        }
        if result.items.is_none() && result.single.is_none() {
            return Err(format!("{name}: `result` needs `items` or `single`"));
        }
        if let Some(p) = &result.items {
            if !p.is_empty() && !p.starts_with('/') {
                return Err(format!("{name}: \"{p}\" is not a JSON pointer"));
            }
        }
        let pointers = result
            .item
            .iter()
            .flat_map(|i| {
                i.name
                    .iter()
                    .chain(i.url.iter())
                    .chain(i.size.iter().flat_map(|s| s.iter()))
            })
            .chain(
                result
                    .single
                    .iter()
                    .flat_map(|s| s.name.iter().chain(s.size.iter().flat_map(|z| z.iter()))),
            );
        for p in pointers {
            if !p.is_empty() && !p.starts_with('/') {
                return Err(format!("{name}: \"{p}\" is not a JSON pointer"));
            }
        }
        if let Some(k) = &result.default_choice_setting {
            if !ctx.settings.contains(k.as_str()) {
                return Err(format!(
                    "{name}: `default_choice_setting` names the undeclared setting \"{k}\""
                ));
            }
        }
    }
    unique(
        &format!("{name} choice"),
        r.choices.iter().map(|c| c.id.as_str()),
    )?;
    for c in &r.choices {
        if !is_valid_key(&c.id) {
            return Err(format!("{name}: the choice \"{}\" is not a valid id", c.id));
        }
        text("a choice label", &c.label, 200)?;
        for d in &c.requires {
            if !ctx.deps.contains(d.as_str()) {
                return Err(format!(
                    "{name}: the choice \"{}\" requires the undeclared dependency \"{d}\"",
                    c.id
                ));
            }
        }
    }
    if let Some(run) = &r.run {
        check_command(
            run,
            Allow {
                url: true,
                choice: true,
                out_dir: true,
            },
            ctx,
        )?;
        if r.choices.is_empty() {
            return Err(format!(
                "{name} has a `run` command but no `choices` to run it with"
            ));
        }
    }
    Ok(())
}

fn check_action(a: &Action, ctx: &Ctx<'_>) -> Check {
    let name = format!("action \"{}\"", a.id);
    if !is_valid_key(&a.id) {
        return Err(format!("{name}: not a valid id"));
    }
    text("an action label", &a.label, 200)?;
    let need = |present: bool, field: &str| -> Check {
        if present {
            Ok(())
        } else {
            Err(format!("{name} needs `{field}`"))
        }
    };
    match a.kind {
        ActionKind::Run => {
            need(a.command.is_some(), "command")?;
            if let Some(c) = &a.command {
                check_command(
                    &Command {
                        command: c.clone(),
                        args: a.args.clone(),
                        timeout_s: a.timeout_s,
                    },
                    Allow::default(),
                    ctx,
                )?;
            }
        }
        ActionKind::Webhook => {
            need(a.webhook.is_some(), "webhook")?;
            if let Some(w) = &a.webhook {
                check_webhook(w, ctx)?;
            }
        }
        ActionKind::Process => {
            if !ctx.has_process {
                return Err(format!(
                    "{name} asks the plugin's process, but the plugin has none"
                ));
            }
        }
        ActionKind::Builtin => need(a.builtin.is_some(), "builtin")?,
        ActionKind::OpenDialog => {
            need(a.dialog.is_some(), "dialog")?;
            if let Some(d) = &a.dialog {
                if !ctx.dialogs.contains(d.as_str()) {
                    return Err(format!("{name} opens the undeclared dialog \"{d}\""));
                }
            }
        }
        ActionKind::OpenUrl => {
            need(a.url.is_some(), "url")?;
            if let Some(u) = &a.url {
                if !is_http(u) {
                    return Err(format!("{name}: \"{u}\" is not an http(s) address"));
                }
            }
        }
    }
    Ok(())
}

fn check_dialog(d: &Dialog, ctx: &Ctx<'_>) -> Check {
    let name = format!("dialog \"{}\"", d.id);
    if !is_valid_key(&d.id) {
        return Err(format!("{name}: not a valid id"));
    }
    text("a dialog title", &d.title, 200)?;
    text("a dialog description", &d.description, 2000)?;
    check_fields(&name, &d.fields, true)?;
    unique(
        &format!("{name} button"),
        d.buttons.iter().map(|b| b.id.as_str()),
    )?;
    for b in &d.buttons {
        if !is_valid_key(&b.id) {
            return Err(format!("{name}: the button \"{}\" is not a valid id", b.id));
        }
        text("a button label", &b.label, 100)?;
        if let Some(a) = &b.action {
            if !ctx.actions.contains(a.as_str()) {
                return Err(format!(
                    "{name}: the button \"{}\" runs the undeclared action \"{a}\"",
                    b.id
                ));
            }
        }
    }
    Ok(())
}

fn check_event(e: &str) -> Check {
    if e == "*" || e == "download.*" || e == "torrent.*" || EVENT_NAMES.contains(&e) {
        Ok(())
    } else {
        Err(format!("\"{e}\" is not an event a hook can listen to"))
    }
}

/// Check a manifest. `Ok` means every later step may rely on its shape.
pub fn validate(m: &Manifest) -> Check {
    if m.api_version == 0 {
        return Err("`api_version` is missing".into());
    }
    if m.api_version > API_VERSION {
        return Err(format!(
            "the plugin needs plugin API {} but this FetchBox supports up to {API_VERSION}: update FetchBox",
            m.api_version
        ));
    }
    if !is_valid_id(&m.id) {
        return Err(format!(
            "\"{}\" is not a valid id (2 to 32 lowercase letters, digits and `-`, starting with a letter)",
            m.id
        ));
    }
    if m.name.trim().is_empty() {
        return Err("`name` is empty".into());
    }
    text("the name", &m.name, 80)?;
    text("the description", &m.description, 2000)?;
    text("the author", &m.author, 200)?;
    if parse_version(&m.version).is_none() {
        return Err(format!(
            "\"{}\" is not a version (like 1.2.3 or 1.2.3-rc.1)",
            m.version
        ));
    }
    if let Some(v) = &m.min_fetchbox {
        if parse_version(v).is_none() {
            return Err(format!("min_fetchbox \"{v}\" is not a version"));
        }
    }
    if let Some(h) = &m.homepage {
        if !is_https(h) {
            return Err("`homepage` must be an https:// address".into());
        }
    }
    for p in &m.platforms {
        if !PLATFORMS.contains(&p.as_str()) {
            return Err(format!("unknown platform \"{p}\" (one of {PLATFORMS:?})"));
        }
    }
    for n in &m.permissions.network {
        if n.is_empty() || n.len() > 253 || n.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(format!("\"{n}\" is not a host pattern"));
        }
    }
    for a in &m.permissions.api {
        if !API_PARTS.contains(&a.as_str()) {
            return Err(format!(
                "unknown API permission \"{a}\" (one of {API_PARTS:?})"
            ));
        }
    }

    // Dependencies.
    if m.dependencies.len() > 16 {
        return Err("more than 16 dependencies".into());
    }
    unique("dependency", m.dependencies.iter().map(|d| d.id.as_str()))?;
    for d in &m.dependencies {
        let name = format!("dependency \"{}\"", d.id);
        if !is_valid_id(&d.id) {
            return Err(format!("{name}: not a valid id"));
        }
        text("a dependency label", &d.label, 200)?;
        if d.platforms.is_empty() == d.github.is_none() {
            return Err(format!(
                "{name} needs either `platforms` or `github`, not both and not neither"
            ));
        }
        if let Some(b) = &d.bin {
            if !is_safe_relative(b) {
                return Err(format!(
                    "{name}: `bin` \"{b}\" must be a path inside the download"
                ));
            }
        }
        for (p, s) in &d.platforms {
            if !PLATFORMS.contains(&p.as_str()) {
                return Err(format!("{name}: unknown platform \"{p}\""));
            }
            if !is_https(&s.url) {
                return Err(format!("{name}: the address for {p} must be https://"));
            }
            if s.sha256.len() != 64 || !s.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "{name}: `sha256` for {p} must be 64 hexadecimal digits"
                ));
            }
            if s.strip > 8 {
                return Err(format!("{name}: `strip` is at most 8"));
            }
        }
        if let Some(g) = &d.github {
            let ok_repo = g.repo.split_once('/').is_some_and(|(o, n)| {
                let part = |s: &str| {
                    !s.is_empty()
                        && s.bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                };
                part(o) && part(n)
            });
            if !ok_repo {
                return Err(format!("{name}: `github.repo` must look like owner/name"));
            }
            if g.assets.is_empty() {
                return Err(format!("{name}: `github.assets` is empty"));
            }
            match &g.sha256_asset {
                Some(a) if !a.is_empty() && !a.contains(['/', '\\']) => {}
                _ => {
                    return Err(format!(
                        "{name}: `github.sha256_asset` is required (the release asset that lists the SHA-256 of the others)"
                    ));
                }
            }
            for (p, a) in &g.assets {
                if !PLATFORMS.contains(&p.as_str()) {
                    return Err(format!("{name}: unknown platform \"{p}\""));
                }
                if a.is_empty() || a.contains(['/', '\\']) {
                    return Err(format!("{name}: \"{a}\" is not a release asset name"));
                }
            }
        }
    }

    let ctx = Ctx {
        settings: m.settings.iter().map(|f| f.key.as_str()).collect(),
        deps: m.dependencies.iter().map(|d| d.id.as_str()).collect(),
        actions: m.ui.actions.iter().map(|a| a.id.as_str()).collect(),
        dialogs: m.ui.dialogs.iter().map(|d| d.id.as_str()).collect(),
        has_process: m.process.is_some(),
    };

    check_fields("settings", &m.settings, false)?;

    if m.resolvers.len() > 16 {
        return Err("more than 16 resolvers".into());
    }
    unique("resolver", m.resolvers.iter().map(|r| r.id.as_str()))?;
    for r in &m.resolvers {
        check_resolver(r, &ctx)?;
    }

    if m.hooks.len() > 32 {
        return Err("more than 32 hooks".into());
    }
    for (i, h) in m.hooks.iter().enumerate() {
        let name = format!("hook {}", i + 1);
        if h.on.is_empty() {
            return Err(format!("{name} listens to no event"));
        }
        for e in &h.on {
            check_event(e).map_err(|e| format!("{name}: {e}"))?;
        }
        if h.every_s == Some(0) {
            return Err(format!("{name}: `every_s` must be at least 1"));
        }
        match &h.action {
            HookAction::Webhook(w) => check_webhook(w, &ctx).map_err(|e| format!("{name}: {e}"))?,
            HookAction::Run(c) => {
                check_command(c, Allow::default(), &ctx).map_err(|e| format!("{name}: {e}"))?;
            }
            HookAction::Process(true) => {
                if !ctx.has_process {
                    return Err(format!(
                        "{name} sends events to the plugin's process, but the plugin has none"
                    ));
                }
            }
            HookAction::Process(false) => return Err(format!("{name}: `process` must be true")),
        }
    }

    if let Some(p) = &m.process {
        check_program(&p.command, &ctx).map_err(|e| format!("process: {e}"))?;
        check_args(&p.args, Allow::default(), &ctx).map_err(|e| format!("process: {e}"))?;
        for (k, v) in &p.env {
            if k.is_empty() || k.contains(['=', '\0']) || k.starts_with("FETCHBOX_") {
                return Err(format!(
                    "process: \"{k}\" cannot be set as an environment variable"
                ));
            }
            check_template(v, Allow::default(), &ctx).map_err(|e| format!("process: {e}"))?;
        }
    }

    // Interface.
    if m.ui.kind == UiKind::Iframe {
        return Err("ui.kind \"iframe\" is not supported in this version".into());
    }
    if m.ui.actions.len() > MAX_ITEMS || m.ui.dialogs.len() > MAX_ITEMS {
        return Err(format!("more than {MAX_ITEMS} actions or dialogs"));
    }
    unique("action", m.ui.actions.iter().map(|a| a.id.as_str()))?;
    unique("dialog", m.ui.dialogs.iter().map(|d| d.id.as_str()))?;
    for a in &m.ui.actions {
        check_action(a, &ctx)?;
    }
    for d in &m.ui.dialogs {
        check_dialog(d, &ctx)?;
    }
    if let Some(t) = &m.ui.tab {
        text("the tab title", &t.title, 80)?;
        for s in &t.sections {
            text("a section title", &s.title, 200)?;
            for k in &s.fields {
                if !ctx.settings.contains(k.as_str()) {
                    return Err(format!("the tab shows the undeclared setting \"{k}\""));
                }
            }
            for a in &s.actions {
                if !ctx.actions.contains(a.as_str()) {
                    return Err(format!("the tab shows the undeclared action \"{a}\""));
                }
            }
        }
    }

    // Translations.
    let texts: usize = m.i18n.values().map(|t| t.len()).sum();
    if texts > 4000 {
        return Err("more than 4000 translated texts".into());
    }
    for (locale, map) in &m.i18n {
        if locale.is_empty() || locale.len() > 16 {
            return Err(format!("\"{locale}\" is not a language code"));
        }
        for (k, v) in map {
            text("a translated text", k, 2000)?;
            text("a translation", v, 2000)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn base() -> serde_json::Value {
        serde_json::json!({
            "api_version": 1, "id": "demo", "name": "Demo", "version": "1.0.0",
            "dependencies": [{
                "id": "tool",
                "platforms": {"linux-x86_64": {"url": "https://example.com/tool", "sha256": SHA}}
            }],
            "settings": [
                {"key": "token", "type": "secret"},
                {"key": "mode", "type": "select", "options": [{"value": "a", "label": "A"}]}
            ],
            "resolvers": [{
                "id": "site", "match": ["https://example.com/*"],
                "probe": {"command": "${dep:tool}", "args": ["-J", "--", "${url}"]},
                "result": {"single": {"name": "/title"}},
                "choices": [{"id": "best", "label": "Best", "args": ["-f", "b"]}],
                "run": {"command": "${dep:tool}", "args": ["${choice.args}", "-P", "${out_dir}", "${url}"]}
            }],
            "ui": {
                "actions": [
                    {"id": "open", "label": "Open", "kind": "open_dialog", "dialog": "d"},
                    {"id": "go", "label": "Go", "kind": "run", "command": "${dep:tool}", "args": ["--version"]}
                ],
                "dialogs": [{"id": "d", "title": "D", "buttons": [{"id": "ok", "label": "OK", "action": "go"}]}]
            }
        })
    }

    fn check(v: serde_json::Value) -> Check {
        validate(&serde_json::from_value::<Manifest>(v).expect("shape"))
    }

    fn err_of(mut v: serde_json::Value, f: impl FnOnce(&mut serde_json::Value)) -> String {
        f(&mut v);
        check(v).expect_err("should be refused")
    }

    #[test]
    fn a_complete_manifest_passes() {
        check(base()).expect("valid");
    }

    #[test]
    fn a_minimal_manifest_passes() {
        check(serde_json::json!({"api_version": 1, "id": "ab", "name": "A", "version": "0.1"}))
            .expect("valid");
    }

    #[test]
    fn a_newer_plugin_api_asks_for_an_update() {
        let e = err_of(base(), |v| v["api_version"] = 2.into());
        assert!(e.contains("update FetchBox"), "{e}");
        assert!(err_of(base(), |v| v["api_version"] = 0.into()).contains("missing"));
    }

    #[test]
    fn the_iframe_ui_is_reserved() {
        let e = err_of(base(), |v| v["ui"]["kind"] = "iframe".into());
        assert!(e.contains("not supported"), "{e}");
    }

    #[test]
    fn ids_and_versions_are_checked() {
        assert!(err_of(base(), |v| v["id"] = "../x".into()).contains("not a valid id"));
        assert!(err_of(base(), |v| v["version"] = "latest".into()).contains("not a version"));
        assert!(err_of(base(), |v| v["homepage"] = "http://x".into()).contains("https"));
        assert!(
            err_of(base(), |v| v["platforms"] = serde_json::json!(["amiga"])).contains("platform")
        );
    }

    #[test]
    fn dependencies_need_https_and_a_checksum() {
        let e = err_of(base(), |v| {
            v["dependencies"][0]["platforms"]["linux-x86_64"]["url"] =
                "http://example.com/t".into();
        });
        assert!(e.contains("https"), "{e}");
        let e = err_of(base(), |v| {
            v["dependencies"][0]["platforms"]["linux-x86_64"]["sha256"] = "abc".into();
        });
        assert!(e.contains("sha256"), "{e}");
        let e = err_of(base(), |v| {
            v["dependencies"][0]["bin"] = "../../etc/passwd".into();
        });
        assert!(e.contains("bin"), "{e}");
        let e = err_of(base(), |v| {
            v["dependencies"][0]["github"] = serde_json::json!({"repo": "o/r", "assets": {"linux-x86_64": "x"}, "sha256_asset": "SUMS"});
        });
        assert!(e.contains("not both"), "{e}");
        let e = err_of(base(), |v| {
            v["dependencies"][0] = serde_json::json!({"id": "tool"});
        });
        assert!(e.contains("not neither"), "{e}");
        let e = err_of(base(), |v| {
            v["dependencies"]
                .as_array_mut()
                .expect("array")
                .push(serde_json::json!({"id": "tool", "github": {"repo": "o/r", "assets": {"linux-x86_64": "x"}, "sha256_asset": "SUMS"}}));
        });
        assert!(e.contains("twice"), "{e}");
    }

    #[test]
    fn a_github_dependency_must_name_its_checksum_asset() {
        let e = err_of(base(), |v| {
            v["dependencies"][0] = serde_json::json!({
                "id": "tool",
                "github": {"repo": "o/r", "assets": {"linux-x86_64": "x"}}
            });
        });
        assert!(e.contains("sha256_asset"), "{e}");
        let mut v = base();
        v["dependencies"][0] = serde_json::json!({
            "id": "tool",
            "github": {"repo": "o/r", "assets": {"linux-x86_64": "x"}, "sha256_asset": "SHA2-256SUMS"}
        });
        check(v).expect("valid");
    }

    #[test]
    fn a_command_cannot_be_a_system_program() {
        for bad in [
            "/bin/sh",
            "sh -c",
            "../../bin/x",
            "${dep:nope}",
            "C:\\x.exe",
            "${url}",
        ] {
            let e = err_of(base(), |v| {
                v["resolvers"][0]["probe"]["command"] = bad.into()
            });
            assert!(
                e.contains("command") || e.contains("dependency"),
                "{bad}: {e}"
            );
        }
        // These are fine.
        for ok in ["bin/tool", "${plugin_dir}/bin/tool", "${dep:tool}"] {
            let mut v = base();
            v["resolvers"][0]["probe"]["command"] = ok.into();
            check(v).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
    }

    #[test]
    fn templates_may_only_use_what_is_declared_and_in_scope() {
        let e = err_of(base(), |v| {
            v["resolvers"][0]["probe"]["args"] = serde_json::json!(["${setting:nope}"]);
        });
        assert!(e.contains("nope"), "{e}");
        // `${out_dir}` and `${choice.args}` belong to `run` only.
        let e = err_of(base(), |v| {
            v["resolvers"][0]["probe"]["args"] = serde_json::json!(["${out_dir}"]);
        });
        assert!(e.contains("out_dir"), "{e}");
        let e = err_of(base(), |v| {
            v["resolvers"][0]["probe"]["args"] = serde_json::json!(["${choice.args}"]);
        });
        assert!(e.contains("choice.args"), "{e}");
        let e = err_of(base(), |v| {
            v["resolvers"][0]["run"]["args"] =
                serde_json::json!([{"when": "dep:ffmpeg", "args": ["x"]}]);
        });
        assert!(e.contains("undeclared dependency"), "{e}");
    }

    #[test]
    fn resolvers_need_a_probe_and_a_result() {
        let e = err_of(base(), |v| {
            v["resolvers"][0]
                .as_object_mut()
                .expect("object")
                .remove("probe");
        });
        assert!(e.contains("probe"), "{e}");
        let e = err_of(base(), |v| {
            v["resolvers"][0]["result"] = serde_json::json!({"items": "/entries"});
        });
        assert!(e.contains("item"), "{e}");
        let e = err_of(base(), |v| {
            v["resolvers"][0]["result"] = serde_json::json!({"single": {"name": "title"}});
        });
        assert!(e.contains("JSON pointer"), "{e}");
        let e = err_of(base(), |v| {
            v["resolvers"][0]["choices"] = serde_json::json!([])
        });
        assert!(e.contains("choices"), "{e}");
        let e = err_of(base(), |v| {
            v["resolvers"][0]["match"] = serde_json::json!([])
        });
        assert!(e.contains("match"), "{e}");
        let e = err_of(base(), |v| {
            v["resolvers"][0]["choices"][0]["requires"] = serde_json::json!(["ffmpeg"]);
        });
        assert!(e.contains("ffmpeg"), "{e}");
    }

    #[test]
    fn hooks_name_real_events_and_have_something_to_run() {
        let e = err_of(base(), |v| {
            v["hooks"] = serde_json::json!([{"on": ["download.exploded"], "action": {"run": {"command": "bin/x"}}}]);
        });
        assert!(e.contains("not an event"), "{e}");
        let e = err_of(base(), |v| {
            v["hooks"] = serde_json::json!([{"on": ["*"], "action": {"process": true}}]);
        });
        assert!(e.contains("has none"), "{e}");
        let e = err_of(base(), |v| {
            v["hooks"] =
                serde_json::json!([{"on": ["*"], "action": {"webhook": {"url": "ftp://x"}}}]);
        });
        assert!(e.contains("webhook"), "{e}");
        let mut v = base();
        v["hooks"] = serde_json::json!([
            {"on": ["download.*", "torrent.completed"], "action": {"webhook": {"url": "https://x.example/h", "method": "POST"}}},
            {"on": ["*"], "action": {"webhook": {"url": "${setting:mode}"}}}
        ]);
        check(v).expect("valid hooks");
    }

    #[test]
    fn the_interface_refers_only_to_what_exists() {
        let e = err_of(base(), |v| v["ui"]["actions"][0]["dialog"] = "ghost".into());
        assert!(e.contains("ghost"), "{e}");
        let e = err_of(base(), |v| {
            v["ui"]["dialogs"][0]["buttons"][0]["action"] = "ghost".into()
        });
        assert!(e.contains("ghost"), "{e}");
        let e = err_of(base(), |v| {
            v["ui"]["tab"] = serde_json::json!({"title": "T", "sections": [{"fields": ["ghost"]}]});
        });
        assert!(e.contains("ghost"), "{e}");
        let e = err_of(base(), |v| {
            v["ui"]["actions"][1] = serde_json::json!({"id": "link", "label": "L", "kind": "open_url", "url": "javascript:alert(1)"});
        });
        assert!(e.contains("http"), "{e}");
        let e = err_of(base(), |v| {
            v["ui"]["actions"][1] =
                serde_json::json!({"id": "go", "label": "L", "kind": "process"});
        });
        assert!(e.contains("has none"), "{e}");
    }

    #[test]
    fn settings_are_sane() {
        let e = err_of(base(), |v| {
            v["settings"][1]["options"] = serde_json::json!([])
        });
        assert!(e.contains("no options"), "{e}");
        let e = err_of(base(), |v| v["settings"][0]["default"] = "x".into());
        assert!(e.contains("secret"), "{e}");
        let e = err_of(base(), |v| v["settings"][0]["type"] = "output".into());
        assert!(e.contains("output"), "{e}");
        let e = err_of(base(), |v| v["settings"][0]["key"] = "Token".into());
        assert!(e.contains("not a valid key"), "{e}");
        let e = err_of(base(), |v| {
            v["settings"][1]["show_if"] = serde_json::json!({"key": "ghost", "equals": true});
        });
        assert!(e.contains("ghost"), "{e}");
    }

    #[test]
    fn the_process_cannot_set_fetchbox_variables() {
        let e = err_of(base(), |v| {
            v["process"] =
                serde_json::json!({"command": "bin/d", "env": {"FETCHBOX_PASSWORD": "x"}});
        });
        assert!(e.contains("environment"), "{e}");
        let mut v = base();
        v["process"] =
            serde_json::json!({"command": "bin/d", "env": {"TOKEN": "${setting:token}"}});
        check(v).expect("valid");
    }
}
