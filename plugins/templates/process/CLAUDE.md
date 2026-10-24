# __NAME__ (FetchBox extension `__ID__`)

A FetchBox extension in a repository of its own. FetchBox reads `plugin.json`, downloads the program from this
repository's releases (a dependency, see below) and keeps it running next to itself, talking to it in JSON lines. The
program is `src/main.rs` (Rust, `serde_json` only). The guide to everything a manifest can do is `plugins/README.md` in the
FetchBox repository (https://github.com/fwitkowski17/FetchBox); read the sections you need before changing the manifest.

## Commands

- Checks (what `ci.yml` runs): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
- Build: `cargo build --release` (the program is `target/release/__ID__`)
- Try it in a FetchBox: `fetchbox --data-dir <data dir of that FetchBox> plugin link .` (from a FetchBox checkout:
  `cargo plugin-link <this folder>`). Then Extensions -> Rescan, review, approve, switch on. After a rebuild press Restart on
  the card; after changing `plugin.json` run `plugin link` again and approve again.
- Talk to the program by hand: `echo '{"id":1,"method":"initialize","params":{"settings":{}}}' | cargo run -q`

## Rules that are easy to break

- **Standard output is the protocol.** One JSON object per line and nothing else: no `println!`, no `dbg!`. Logs go out as
  the `log` notification or to standard error (the card's "Show log"). Lines are at most 1 MiB.
- Every message with an `id` gets exactly one answer with that `id`; a notification (no `id`) gets none. `initialize` must be
  answered within 10 s, `invoke` and `resolve` within 60 s. Do not block the loop on slow work.
- `permissions.uses` in `plugin.json` must list exactly the groups the manifest really uses (`network`, `programs`,
  `background`, `events`, `downloads`, `api`), each with a `reasons` sentence of at most 200 characters, or FetchBox refuses
  the manifest. Adding a hook, a dependency or a webhook can change the set.
- Any change to `plugin.json` puts an installed extension back to "needs review" (the user approves the new hash).
- **Keep these in step:** the version in `plugin.json` and `Cargo.toml` (the release tag `v<version>` must equal both); the
  program name = the extension id = the dependency id = `[[bin]] name`; the dependency's `assets`, `platforms` and the `matrix`
  of `.github/workflows/release.yml` (`__ID__-<os>-<arch>` for each platform).
- The program runs without a shell, with a minimal environment (no FetchBox secrets) and, in the official Docker image, with
  no libc of its own to match: Linux builds are static (musl). Do not add a dependency that needs a C toolchain (TLS libraries
  such as ring or aws-lc) without setting that up in `release.yml` first.
- Write only below `$FETCHBOX_PLUGIN_DATA`; the extension's own folder is read-only code that is replaced on update.
- Texts shown to the user (labels, help, descriptions) are plain text, never HTML; translations go in `i18n`, keyed by the
  English text.

## Releasing

Set the real `owner/name` in `plugin.json` (`github.repo`, `homepage`; it is `__REPO__` now), bump the version in both files,
commit, then `git tag v<version> && git push origin v<version>`. `release.yml` builds the programs and publishes `SHA2-256SUMS`
and `__ID__-<version>.zip`. Details in README.md.
