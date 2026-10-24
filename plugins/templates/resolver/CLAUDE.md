# __NAME__ (FetchBox extension `__ID__`)

A FetchBox extension in a repository of its own. It teaches **Add download** about addresses: when the user pastes an address
that matches `resolvers[].match` in `plugin.json`, FetchBox runs the program of this project, reads the JSON it prints and
shows the entries as a list; FetchBox's own engine downloads each entry's `url`. The program is `src/main.rs` (Rust,
`serde_json` only). The guide to everything a manifest can do is `plugins/README.md` in the FetchBox repository
(https://github.com/fwitkowski17/FetchBox); read the Resolvers section before changing the manifest.

## Commands

- Checks (what `ci.yml` runs): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
- Build: `cargo build --release` (the program is `target/release/__ID__`)
- Run it as FetchBox does: `./target/release/__ID__ 'https://example.com/img[001-003].jpg' 10`
- Try it in a FetchBox: `fetchbox --data-dir <data dir of that FetchBox> plugin link .` (from a FetchBox checkout:
  `cargo plugin-link <this folder>`). Then Extensions -> Rescan, review, approve, switch on, add a pattern under Sites in
  Settings -> Extensions. After a rebuild nothing else is needed (the program starts again for every address); after changing
  `plugin.json` run `plugin link` again and approve again.

## The contract of the program

- Arguments are what `resolvers[].probe.args` says (`${url}`, `${setting:key}`); the program is started once per address,
  without a shell, with a minimal environment, and killed after `timeout_s` (at most 120 s).
- It prints **one JSON document** on standard output (at most 8 MiB) and exits with 0. `result` in `plugin.json` says where
  the list (`items`) and each entry's `name`, `url` and `size` are, as JSON Pointers: change the output and the pointers together.
- A failure is a non-zero exit code with the reason on standard error (the last lines are shown to the user). It never
  blocks adding the address in the ordinary way.
- Without `run` in the resolver the entries are plain addresses for FetchBox's engine. To make the file with your own program,
  add `run` and `choices` (see `plugins/official/yt-dlp` in the FetchBox repository).

## Rules that are easy to break

- `permissions.uses` in `plugin.json` must list exactly the groups the manifest really uses (`network`, `programs`,
  `background`, `events`, `downloads`, `api`), each with a `reasons` sentence of at most 200 characters, or FetchBox refuses
  the manifest. Adding `run` adds `downloads`.
- Any change to `plugin.json` puts an installed extension back to "needs review" (the user approves the new hash).
- **Keep these in step:** the version in `plugin.json` and `Cargo.toml` (the release tag `v<version>` must equal both); the
  program name = the extension id = the dependency id = `[[bin]] name`; the dependency's `assets`, `platforms` and the `matrix`
  of `.github/workflows/release.yml` (`__ID__-<os>-<arch>` for each platform).
- In the official Docker image there is no libc of its own to match: Linux builds are static (musl). The program has no network
  code on purpose; do not add a dependency that needs a C toolchain (TLS libraries such as ring or aws-lc) without setting that up
  in `release.yml` first.
- Write only below `$FETCHBOX_PLUGIN_DATA`; the extension's own folder is read-only code that is replaced on update.
- Texts shown to the user are plain text, never HTML; translations go in `i18n`, keyed by the English text.

## Releasing

Set the real `owner/name` in `plugin.json` (`github.repo`, `homepage`; it is `__REPO__` now), bump the version in both files,
commit, then `git tag v<version> && git push origin v<version>`. `release.yml` builds the programs and publishes `SHA2-256SUMS`
and `__ID__-<version>.zip`. Details in README.md.
