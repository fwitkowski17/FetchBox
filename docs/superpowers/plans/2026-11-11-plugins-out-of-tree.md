# Plugins out of tree Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the video extension out of FetchBox as `youtube-downloader`, remove shared programs, and warn about plugins made for an older plugin API.

**Architecture:** Deletions first (bundled `yt-dlp`, then the `shared` dependency form end to end), then a pure compatibility rule in `core::plugin` surfaced as `compat` on `PluginView`/`StagedPlugin` and drawn by the web UI, then the standalone plugin project, validated by the FetchBox built here.

**Tech Stack:** Rust workspace (core, plugins, server), utoipa → `docs/openapi.json` → `openapi-typescript`, Svelte 5 + TS.

**Spec:** `docs/superpowers/specs/2026-11-11-plugins-out-of-tree-design.md`

## Global Constraints

- Branch `plugins-out-of-tree` (from `develop`). Code, comments, docs, commits in English; the user is answered in Polish.
- No `unwrap()`/`expect()` outside tests. `thiserror` in libraries. No new dependencies.
- After every task: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` green; tasks touching `web/` also `cd web && npm run check && npm test`.
- API changes → `scripts/gen-api.sh`; never hand-edit `web/src/lib/api/`.
- `API_VERSION = 2`, `MIN_API_VERSION = 1` (in `crates/core/src/plugin.rs`).
- Removed-`shared` message, verbatim: `` `shared` was removed in plugin API 2; declare the program with `platforms` or `github` ``.
- Plugin name `YouTube Downloader` (no `i18n` entry for the name), id `youtube-downloader`, version `0.1.0`, `api_version` 2, homepage `https://github.com/fwitkowski17/youtube-downloader`.

## Review Focus

- A manifest still carrying `"shared": "ffmpeg"` (an old yt-dlp ZIP on a user's disk, a folder in `<data>/plugins/`): must be refused with the message above, not a generic "needs exactly one of" — Task 2 test.
- An installed v1 plugin after upgrading FetchBox (e.g. a user's own `webhook-notify`-like hook with `api_version: 1`): must keep running, with the warning — Task 3 server test.
- Data dirs with a leftover `<data>/plugin-programs/` folder: must be ignored, startup not failing — Task 2 test.
- The store with only `webhook-notify` bundled and an empty `index.json`: the Store tab and `/api/plugin-store` must still answer `200` with one entry — Task 1 (existing HTTP test updated).
- `plugin link` on youtube-downloader (two non-shared deps): must work with `--dependency yt-dlp --bin …` — Task 5 verification.

---

### Task 1: Remove the bundled `yt-dlp` extension

**Files:**
- Delete: `plugins/official/yt-dlp/`
- Modify: `crates/server/src/manager/plugin_store.rs:280,286`, `crates/server/src/tests_plugins.rs:769-858` (`the_review_lists_dependencies_apart_and_says_what_will_be_downloaded`), `:1862-1866`
- Modify docs: `docs/SPEC.md` (§2 Extensions bullet: "the reference extension is YouTube…" → extensions live in their own repositories; §6.15 intro "one reference extension (`yt-dlp`)"; "Extensions that ship" → only `webhook-notify`, mention youtube-downloader as an external project; the tests note "The `yt-dlp` extension was run by hand…" removed), `plugins/README.md:32-34` and every link to `official/yt-dlp`, `plugins/templates/resolver/README.md:40`, `plugins/templates/resolver/CLAUDE.md:28` (point at `https://github.com/fwitkowski17/youtube-downloader`), `CONTRIBUTING.md:57`, top-level `README.md` where it lists bundled extensions (grep `yt-dlp`).

**Interfaces:** Produces nothing new. After this task `Bundled` holds only `webhook-notify`.

- [ ] **Step 1: Update the expectations** — in `plugin_store.rs` both asserts become `["webhook-notify"]`; in `tests_plugins.rs:1864` `["webhook-notify"]`.
- [ ] **Step 2: Rewrite the dependency-plan test without the store.** Keep its name. Stage with `rig.mgr.plugin_stage(package(&m, false))` where
  `m = manifest("deps", json!({"permissions": {...declared...}, "dependencies": [{"id": "tool", "github": {"repo": "o/r", "assets": {<platform_key()>: "tool"}, "sha256_asset": "SHA2-256SUMS"}}, {"id": "extra", "optional": true, "platforms": {<platform_key()>: {"url": "https://example.com/extra", "sha256": "<64×a>"}}}]}))` (use `declared()` as the other tests do). Assert:
  `deps == [("extra", DependencySource::Pinned, true), ("tool", DependencySource::Github, false)]` (sorted as the summary sorts — check the existing order rule and match it), the "not a program it runs" assert unchanged, `plan(tool) == Download`, `plan(extra) == OnRequest`. Drop everything about `plugin-programs` and `shared_program` (Task 2 removes them).
- [ ] **Step 3: Delete `plugins/official/yt-dlp/`.**
- [ ] **Step 4: Run** `cargo test -p server` — Expected: PASS (the shared-program test at `:680` still passes; it builds its own manifest).
- [ ] **Step 5: Docs** as listed under Files; `grep -rn "official/yt-dlp" . --exclude-dir=target --exclude-dir=.git` returns nothing.
- [ ] **Step 6: Commit** `Move the yt-dlp extension out of FetchBox`.

### Task 2: Remove shared programs

**Files:**
- Delete: `plugins/programs.json`, `crates/core/src/plugin/programs.rs`
- Modify: `crates/core/src/plugin.rs` (mod + re-exports of `programs`), `crates/core/src/plugin/manifest.rs:185-206`, `crates/core/src/plugin/validate.rs:600-630` and test `:1051`, `crates/core/src/plugin/view.rs:113-220` and test `:632`, `crates/plugins/src/deps.rs` (`shared` field of `Installer`, `with_shared_programs`, `shared_entry`, the shared arms of `dep_dir`/`bin_path`/`state`/`install`/`plan`, `was_installed_before`, legacy-copy cleanup, tests `:750-1000` that are about shared), `crates/plugins/src/install.rs:54` (`Layout::programs_dir`), `crates/plugins/tests/common/mod.rs`, `crates/server/src/tests_plugins.rs:676-768` (`plugins_share_a_program_from_fetchboxs_list` — delete), `web/src/lib/util/dependencies.test.ts:59` (use `source: 'pinned'`), any web code switching on `'shared'`.
- Regenerate: `docs/openapi.json`, `web/src/lib/api/schema.d.ts` via `scripts/gen-api.sh`.
- Docs: `docs/SPEC.md` §5 (`programs.json`, `programs` in the core file list, `plugin-programs` path), §6.15 "Shared programs" bullet removed, "Dependencies" bullet: "exactly one source: `platforms` or `github`", a line that `<data>/plugin-programs/` from older versions is no longer read and may be deleted by hand; `plugins/README.md` (the `shared` section and table rows); `plugins/templates/{resolver,process}/CLAUDE.md` (shared mentions).

**Interfaces:**
- `Dependency` loses `pub shared: Option<String>` and gains `#[serde(default, rename = "shared", skip_serializing)] pub removed_shared: Option<serde_json::Value>` with a doc comment saying it exists only for the error message.
- `DependencySource` = `Pinned | Github`.
- `Installer` loses `with_shared_programs`, `was_installed_before`. `plan(plugin, dep)`: required → `Download` unless present; optional → `OnRequest` unless present.

- [ ] **Step 1: Failing test** in `validate.rs` replacing `:1051`:
  ```rust
  #[test]
  fn a_shared_dependency_is_refused_with_what_to_do_instead() {
      let e = err_of(base(), |v| v["dependencies"][0] = json!({"id": "ffmpeg", "optional": true, "shared": "ffmpeg"}));
      assert!(e.contains("`shared` was removed in plugin API 2; declare the program with `platforms` or `github`"), "{e}");
  }
  ```
- [ ] **Step 2: Run** `cargo test -p fetchbox-core a_shared_dependency_is_refused` — Expected: FAIL (today it validates against the list).
- [ ] **Step 3: Implement** the field change and the check (first check of each dependency, before the "exactly one of" count, which now counts `platforms` and `github`). Remove `programs.rs`, its `include_str!` and every user; fix `view.rs` (`dependency_need` no longer looks up a list; label = manifest label else id); strip `deps.rs` and `install.rs` as listed; delete the server and plugins-crate shared tests.
- [ ] **Step 4: Leftover folder test** in `deps.rs` tests: create `<data>/plugin-programs/ffmpeg/x/` in the temp layout, then a pinned dependency installs and `state` is correct — proves the folder is ignored.
- [ ] **Step 5: Run** `cargo test --workspace` and clippy — Expected: PASS, no dead-code warnings.
- [ ] **Step 6:** `scripts/gen-api.sh`; fix `dependencies.test.ts`; `cd web && npm run check && npm test` — PASS. `grep -rn "shared_program\|programs.json\|plugin-programs\|DependencySource::Shared" crates web/src plugins docs` returns only the SPEC line about the old folder.
- [ ] **Step 7: Docs**, then **Commit** `Remove shared programs: every plugin declares its own`.

### Task 3: Plugin API compatibility (backend)

**Files:**
- Modify: `crates/core/src/plugin.rs:34-35`, `crates/core/src/plugin/validate.rs:541-548`, `crates/core/src/plugin/view.rs` (`PluginView`, `StagedPlugin`), `crates/server/src/manager/plugins.rs:212-222` (`unsupported`), `:577`, `:649`, `:1065` (fill `compat`), `plugins/templates/*/plugin.json` and `plugins/official/webhook-notify/plugin.json` (`api_version: 2`), test manifests in `tests_plugins.rs`/`validate.rs`/`view.rs` that use `"api_version": 1` stay as they are (they now exercise the older path; only fix asserts that break).
- Regenerate API types.
- Docs: `plugins/README.md:195-205` (`api_version` is `2`; what 2 changed — `shared` removed; v1 manifests run with a warning; the minimum), `docs/SPEC.md` §6.15 (State / validation: the compatibility rule and `compat`).

**Interfaces (produced):**
```rust
// crates/core/src/plugin.rs
pub const API_VERSION: u32 = 2;
pub const MIN_API_VERSION: u32 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiCompat { Current, Older, TooOld }
pub fn api_compat(api_version: u32) -> ApiCompat;            // uses the two consts
fn api_compat_in(api_version: u32, min: u32, current: u32) -> ApiCompat; // pure, tested
// crates/core/src/plugin/view.rs  (Serialize, Deserialize, ToSchema under "openapi")
pub struct CompatWarning { pub api_version: u32, pub current: u32 }
// PluginView and StagedPlugin: pub compat: Option<CompatWarning>   (Some only for ApiCompat::Older)
pub fn compat_warning(api_version: u32) -> Option<CompatWarning>;
```
`validate` keeps refusing `0` and `> API_VERSION`. `unsupported()` returns `Some("it was made for plugin API {v}; this FetchBox supports {MIN_API_VERSION} to {API_VERSION}")` for `TooOld`.

- [ ] **Step 1: Failing tests** in `crates/core/src/plugin.rs`:
  `api_compat_in(4,2,4)==Current`, `api_compat_in(3,2,4)==Older`, `api_compat_in(2,2,4)==Older`, `api_compat_in(1,2,4)==TooOld`; `compat_warning(API_VERSION).is_none()`, `compat_warning(1) == Some(CompatWarning{api_version:1, current:2})`.
- [ ] **Step 2: Run** `cargo test -p fetchbox-core api_compat` — FAIL (not defined).
- [ ] **Step 3: Implement** the consts, enum, functions, the view fields and their filling in `plugins.rs` (from the manifest's `api_version`), and the `TooOld` branch of `unsupported()`.
- [ ] **Step 4: Server test** in `tests_plugins.rs`: `an_older_plugin_api_still_runs_with_a_warning` — `rig.install(&manifest("old", json!({})), false)` with `api_version` 1 (what `manifest()` produces — confirm, else set it); assert `view.compat == Some(CompatWarning{api_version:1, current:API_VERSION})`, enable it, `wait_for("old", PluginState::Ready)`. Same file: staging that manifest returns `staged.compat.is_some()`. And a manifest with `api_version: API_VERSION` has `compat == None`.
- [ ] **Step 5: Run** `cargo test --workspace`, clippy — PASS.
- [ ] **Step 6:** `scripts/gen-api.sh`; `cd web && npm run check` — PASS.
- [ ] **Step 7: Docs**, **Commit** `Plugin API 2: warn about plugins made for an older API`.

### Task 4: Compatibility note in the web UI

**Files:**
- Modify: `web/src/lib/components/PluginCard.svelte`, `web/src/lib/components/InstallPluginDialog.svelte`, `web/src/lib/i18n/en.ts`, `web/src/lib/i18n/pl.ts` (and every other locale file the key test requires).
- Test: `web/src/lib/util/plugins.test.ts`

**Interfaces:** Consumes `compat: CompatWarning | null` on `PluginView` and `StagedPlugin` (generated types). Produces `compatNote(compat: CompatWarning | null | undefined): { version: number } | null` in `web/src/lib/util/plugins.ts`.

- [ ] **Step 1: Failing test** in `plugins.test.ts`: `compatNote(null) === null`, `compatNote(undefined) === null`, `compatNote({api_version: 1, current: 2})` equals `{ version: 1 }`.
- [ ] **Step 2: Run** `cd web && npx vitest run src/lib/util/plugins.test.ts` — FAIL.
- [ ] **Step 3: Implement** `compatNote`; key `'ext.compat.older'`:
  en `'Made for an older version of FetchBox (plugin API {version}). It may not work correctly; look for an update.'`
  pl `'Zbudowany dla starszej wersji FetchBox (API pluginów {version}). Może nie działać poprawnie, poszukaj aktualizacji.'`
  Draw it in `PluginCard` under the description and in `InstallPluginDialog` above the permission list, as an amber note using the same warning style the card already uses for problems (reuse its classes/icon; rendered as text). Install stays enabled.
- [ ] **Step 4: Run** `npm run check && npm test` — PASS.
- [ ] **Step 5: Commit** `Show a note on plugins made for an older plugin API`.

### Task 5: The youtube-downloader project

**Files (outside this repo, `~/Projekty`):**
- Rename: `~/Projekty/yt-dlp` → `~/Projekty/youtube-downloader`
- Modify: `plugin.json`, `README.md`, `CLAUDE.md` there.

**Interfaces:** Consumes the FetchBox built from this branch (API 2, no `shared`).

- [ ] **Step 1:** `mv ~/Projekty/yt-dlp ~/Projekty/youtube-downloader`.
- [ ] **Step 2: `plugin.json`** — set `id`, `name`, `version`, `api_version`, `homepage` per Global Constraints; `author` = `git config user.name` (fallback "FetchBox"). Replace the ffmpeg dependency with `{"id": "ffmpeg", "label": "ffmpeg (merges video and audio, makes MP3)", "optional": true, "platforms": {…}}` where `platforms` is the `platforms` object of the `ffmpeg` entry of the deleted `plugins/programs.json` copied verbatim (get it with `git show develop:plugins/programs.json | jq '.programs[0].platforms'`). Remove the `i18n.pl` key for the old name (`"Video sites (yt-dlp)"`); keep the rest. Keep the description and the sites it names.
- [ ] **Step 3: README/CLAUDE.md** — new id/name everywhere; "carries its own ffmpeg (static imageio-ffmpeg 0.6.0 builds, pinned by SHA-256)"; trying it: copy `plugin.json` to `<data>/plugins/youtube-downloader/` or `fetchbox --data-dir <data> plugin link . --dependency yt-dlp --bin "$(command -v yt-dlp)"`; release tag `v0.1.0`; CLAUDE.md rule "the id stays `youtube-downloader`" and "`api_version` 2".
- [ ] **Step 4: Verify** with this branch's FetchBox:
  `D=$(mktemp -d) && cargo run -q -p server -- --data-dir $D plugin link ../youtube-downloader --dependency yt-dlp --bin /bin/true`
  Expected: `Linked youtube-downloader into …`; `cmp` of the copied manifest succeeds. Also `jq empty plugin.json`.
- [ ] **Step 5: Commit** in that repo: `Rename to youtube-downloader; carry ffmpeg; plugin API 2`.

### Task 6: Whole-branch verification

- [ ] `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` — PASS.
- [ ] `cd web && npm run check && npm test` — PASS.
- [ ] `grep -rn "yt-dlp" docs/SPEC.md plugins/README.md` — only mentions of the external project / sample strings.
- [ ] Spec success criteria re-read and ticked; leave the branch unmerged for the user.
