# __NAME__

A FetchBox extension that teaches **Add download** about the addresses you list. Paste an address, FetchBox runs the
program of this project, and what it prints appears as a list of files to choose from; the files themselves are
downloaded by FetchBox's own engine. The example expands an address with a numbered range, such as
`https://example.com/img[001-020].jpg`, into one entry per number. It is written in Rust (`src/main.rs`) and the project is
meant to live in a repository of its own:

```
plugin.json                      the manifest FetchBox reads
CLAUDE.md                        notes for Claude Code (or any assistant) working on this project
src/main.rs                      the program (reads the address, prints JSON, a few tests)
Cargo.toml                       builds the program as `__ID__`
.github/workflows/ci.yml         fmt, clippy and tests on every push
.github/workflows/release.yml    on a tag: static programs, SHA2-256SUMS and the plugin ZIP
```

## Develop

```sh
cargo test
cargo build --release
./target/release/__ID__ 'https://example.com/img[001-003].jpg' 10      # what FetchBox runs
fetchbox --data-dir <data dir of your FetchBox> plugin link .
# or, from the root of a FetchBox checkout (dev backend): cargo plugin-link path/to/__ID__
```

`plugin link` copies `plugin.json` into `<data dir>/plugins/__ID__/` and links `target/release/__ID__` where the
dependency in `plugin.json` would have been downloaded, so nothing is fetched before the first release exists. Then
**Extensions → Rescan**, review the permissions, approve and switch it on. In **Settings → Extensions** add a pattern under
*Sites*, for example `https://example.com/*`, and save. Paste `https://example.com/img[001-020].jpg` into **Add download**.

After a change to the program, `cargo build --release` is enough (it is started again for every address). After a change to
`plugin.json`: run `plugin link` again and approve the extension again.

## Where to go from here

- `result` in `plugin.json` maps the JSON the program prints into the list (JSON Pointers); add `size` when you know it.
- Make the file with your own program instead of the engine: add `run` and `choices` to the resolver (see
  `plugins/official/yt-dlp` in the FetchBox repository).
- The program has no network code on purpose: a TLS library in a static program built for several platforms needs a C
  toolchain for each, which this workflow does not set up. Use a pure-Rust client, or add another program as a dependency
  and have the resolver run that instead.

## Publish

1. Create the repository and set it in `plugin.json`: `github.repo` of the dependency and `homepage` must be the
   `owner/name` of the repository that holds the releases (`__REPO__` now).
2. Push, then tag a release. The tag is the version in `plugin.json` and `Cargo.toml`:
   `git tag v0.1.0 && git push origin v0.1.0`.
3. The workflow builds the program for Linux (x86_64 and aarch64, static, so it also runs in the official Docker image)
   and macOS (Apple silicon), and publishes the release with `SHA2-256SUMS` and `__ID__-0.1.0.zip`. FetchBox downloads
   the program from that release and checks it against `SHA2-256SUMS`. Another platform is a line in the `matrix` of
   `release.yml`, a line in `assets` and one in `platforms` of `plugin.json`.
4. Users install the ZIP with **Extensions → Install from file**. To list it in the store, add an entry to `plugins/index.json`
   of FetchBox with the address of the ZIP and its SHA-256 (the job summary of the release prints it); see
   `plugins/README.md` in the FetchBox repository.

The installed manifest is pinned to its release and to the exact programs in it (`github.tag` and `github.sha256`, the
SHA-256 of each program, are written into the ZIP's `plugin.json`), so an old version of the extension never downloads a
newer program and an asset swapped after publishing is refused. Do not write these two by hand.

The full guide to what a manifest can do is `plugins/README.md` in the FetchBox repository.
