# Plugins out of tree: youtube-downloader, no shared programs, API compatibility

Date: 2026-11-11. Status: approved in conversation, awaiting review of this document.

## Intent

FetchBox extensions are meant to live in repositories of their own. The video extension still ships inside FetchBox
(`plugins/official/yt-dlp`) and leans on a FetchBox-only mechanism (`shared` programs from `plugins/programs.json`), which
ties an out-of-tree plugin to FetchBox's release cycle. The maintainer wants:

1. The video extension out of FetchBox, as its own project `youtube-downloader` (uses the yt-dlp program).
2. Every plugin to carry its own dependencies (declared in its manifest), so building and distributing a plugin needs
   nothing from FetchBox. A plugin may pin whatever build it needs (for example an audio-only ffmpeg).
3. A way to tell the user that an installed plugin was made for an older plugin API and may not work, when FetchBox's
   plugin format changes.

Considered and dropped: a "core" ffmpeg plugin that other plugins depend on (plugin-to-plugin dependencies). It adds
install ordering, protection against removal and a new dependency source for little gain: merging streams and making MP3
are cheap (remux, audio encode), and ~70 MB of duplicated ffmpeg per plugin is acceptable on a homelab. Remote
transcoding is out of scope.

## Success criteria

- `plugins/official/` holds only `webhook-notify`; the store still works offline with it; all tests pass
  (`cargo test --workspace`, clippy `-D warnings`, fmt, `npm run check`, web tests).
- `~/Projekty/youtube-downloader` is a standalone git project whose `plugin.json` validates in FetchBox, declares yt-dlp
  (`github`) and ffmpeg (`platforms`, pinned) itself, and can be tried with `plugin link`.
- No `shared` dependency form, no `plugins/programs.json`, no `plugin-programs/` handling remain in FetchBox.
- A plugin with `api_version` older than the current one but not older than the minimum installs and runs, and the UI
  shows a warning on its card and in the install dialog; one older than the minimum is `unsupported` with a reason.

## Part 1: remove `plugins/official/yt-dlp` from FetchBox

- Delete `plugins/official/yt-dlp/`.
- Tests that name it: `crates/server/src/manager/plugin_store.rs` (bundled ids/names), `crates/server/src/tests_plugins.rs`
  (store staging of `yt-dlp` with its dependency plan around lines 785–853, bundled list at ~1864). Rewrite them against
  `webhook-notify` or a test manifest built in the test; the dependency-plan test keeps its coverage with a manifest of its
  own served from the local test server (the test already uses one for downloads).
- Tests that only use the string `yt-dlp` as sample data (`template.rs`, `text.rs`, `deps.rs`, `archive.rs`,
  `storage/downloads.rs`, web `*.test.ts`) stay: they do not depend on the bundled plugin.
- Docs: `docs/SPEC.md` (2. Functional requirements, 6.15 intro, "Extensions that ship", "Shared programs", tests note),
  `plugins/README.md` (the "two extensions in official/" paragraph, examples pointing at `official/yt-dlp`),
  `plugins/templates/resolver/{README,CLAUDE}.md` (point at the youtube-downloader repository instead),
  `CONTRIBUTING.md`, top-level `README.md` where it lists bundled extensions.
- `plugins/index.json` stays empty; youtube-downloader is added there after its first release (not part of this work).

## Part 2: remove shared programs

- Delete `plugins/programs.json`, `crates/core/src/plugin/programs.rs` and its `include_str!`.
- `Dependency.shared` and `DependencySource::Shared` go away. A manifest that still has `shared` fails validation with a
  clear message ("`shared` was removed in plugin API 2; declare the program with `platforms` or `github`"). The manifest
  structs do not deny unknown fields, so a dropped field would be silently ignored and the user would only see "needs
  exactly one of `platforms` or `github`". `Dependency` therefore keeps a private-purpose
  `#[serde(default, skip_serializing)] removed_shared: Option<serde_json::Value>` (renamed from `shared` via
  `#[serde(rename = "shared")]`), read only by `validate` to give the message above.
- `crates/plugins/src/deps.rs`: drop `shared_entry`, `with_shared_programs`, the shared branch of `dep_dir`/`bin_path`/
  `state`/`install`/`plan`, `was_installed_before` and the legacy-copy cleanup, `Layout::programs_dir`. Their tests go too.
- `validate.rs`, `derive_permissions`/`used_groups`, `install.rs`, `crates/plugins/tests/common`, `tests_plugins.rs`: remove
  shared cases.
- API: `DependencySource` loses `shared` → regenerate `docs/openapi.json` and `web/src/lib/api/schema.d.ts`
  (`scripts/gen-api.sh`); the web `DependencyList`/`util/dependencies` drop the shared label and its test cases.
- Existing installs: `<data>/plugin-programs/` is no longer read. Nothing deletes it; SPEC says it can be removed by hand.

## Part 3: plugin API compatibility

- `crates/core/src/plugin.rs`: `API_VERSION = 2` (Part 2 is a breaking change of the format) and new
  `MIN_API_VERSION = 1`.
- Rules (pure functions in `core::plugin`, unit-tested):
  - `api_version == 0` → invalid (as today); `> API_VERSION` → refused (as today).
  - `< MIN_API_VERSION` → valid manifest but `unsupported`, reason "it was made for plugin API N, this FetchBox supports
    MIN–CUR".
  - `MIN_API_VERSION <= api_version < API_VERSION` → runs, with a compatibility warning.
- Note: a v1 manifest that uses `shared` is invalid anyway (Part 2). A v1 manifest without it runs with the warning.
- View: `PluginView` and `StagedPlugin` get `compat: Option<CompatWarning>` where
  `CompatWarning { api_version: u32, current: u32 }` (English text is built in the UI through i18n, not sent by the
  server). OpenAPI and TS types regenerated.
- `unsupported()` in `server/src/manager/plugins.rs` gains the `MIN_API_VERSION` check next to platform and
  `min_fetchbox`.
- Frontend: `PluginCard` shows an amber note "Made for an older version of FetchBox (plugin API N). It may not work
  correctly; look for an update." `InstallPluginDialog` shows the same note above the permissions; install is not
  blocked. Translations in `en.ts` and `pl.ts`.
- `plugins/README.md`: document `api_version` 2, what changed from 1, and the warning; templates in
  `plugins/templates/*/plugin.json` move to `api_version: 2`; `webhook-notify` too.
- `min_fetchbox` stays as it is (the lower bound on FetchBox itself).
- The store catalog (`index.json`) is unchanged; the warning appears once a package is staged.

## Part 4: the youtube-downloader project (`~/Projekty/youtube-downloader`)

- Rename the folder `~/Projekty/yt-dlp` → `~/Projekty/youtube-downloader` (its git history kept).
- `plugin.json`: `id` `youtube-downloader`, `name` "YouTube downloader" (pl: "Pobieranie z YouTube"), `version` `0.1.0`,
  `api_version` 2, `homepage` `https://github.com/fwitkowski17/youtube-downloader`, `author` from git config.
  Dependencies: `yt-dlp` (`github`, unchanged), `ffmpeg` (`optional`, `platforms` with the four imageio-ffmpeg 0.6.0
  wheels, `sha256`, `size`, `archive: zip` and per-platform `bin`, values copied from `programs.json`).
  Everything else (resolver, choices, settings, i18n, the update action) unchanged. Description and i18n keep naming
  the other sites it supports.
- Because the dependency `yt-dlp` no longer shares the plugin's id, `plugin link` without `--dependency` would complain
  about several dependencies; README/CLAUDE.md document `plugin link . --dependency yt-dlp --bin "$(command -v yt-dlp)"`
  or copying `plugin.json` by hand.
- README.md, CLAUDE.md updated to the new id and to "carries its own ffmpeg"; release workflow already names the ZIP by
  id. Commit in that repository.

## Testing

- Rust: unit tests for the compatibility rules; validation of `shared` refusal; server test that a v1 manifest installs
  and reports `compat`, and one below the minimum is `unsupported` (needs `MIN_API_VERSION` overridable in tests or a
  test that reasons about the pure function only, preferring the latter); store tests against `webhook-notify`.
- Web: unit tests for the note's visibility in `util/plugins` (or wherever the card logic lives); `npm run check`.
- youtube-downloader: validated through `fetchbox plugin link` into a scratch data dir.

## Out of scope

Plugin-to-plugin dependencies, a core ffmpeg plugin, remote transcoding, a `tested_fetchbox` field, cleanup of old
`plugin-programs/` folders, publishing youtube-downloader to the store.
