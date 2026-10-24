# __NAME__

A FetchBox extension: a program that runs next to FetchBox. FetchBox starts it with the extension, keeps it running,
sends it the events of a hook and lets a button ask it something. This is the shape for a metrics exporter or an MCP
bridge. It is written in Rust (`src/main.rs`) and the project is meant to live in a repository of its own:

```
plugin.json                      the manifest FetchBox reads
CLAUDE.md                        notes for Claude Code (or any assistant) working on this project
src/main.rs                      the program (the protocol, a few tests)
Cargo.toml                       builds the program as `__ID__`
.github/workflows/ci.yml         fmt, clippy and tests on every push
.github/workflows/release.yml    on a tag: static programs, SHA2-256SUMS and the plugin ZIP
```

## Develop

```sh
cargo test
cargo build --release
fetchbox --data-dir <data dir of your FetchBox> plugin link .
# or, from the root of a FetchBox checkout (dev backend): cargo plugin-link path/to/__ID__
```

`plugin link` copies `plugin.json` into `<data dir>/plugins/__ID__/` and links `target/release/__ID__` where the
dependency in `plugin.json` would have been downloaded, so nothing is fetched before the first release exists. Then
**Extensions → Rescan**, review the permissions, approve and switch it on; the card shows the line "ready". Press **Ping**,
let a download finish and open **Show log**.

After a change to the program: `cargo build --release` and **Restart** on the card. After a change to `plugin.json`: run
`plugin link` again and approve the extension again.

You can also talk to the program by hand, the protocol is plain lines:

```sh
echo '{"id":1,"method":"initialize","params":{"settings":{"greeting":"hi"}}}' | cargo run -q
```

## Publish

1. Create the repository and set it in `plugin.json`: `github.repo` of the dependency and `homepage` must be the
   `owner/name` of the repository that holds the releases (`__REPO__` now).
2. Push, then tag a release. The tag is the version in `plugin.json` and `Cargo.toml`:
   `git tag v0.1.0 && git push origin v0.1.0`.
3. The workflow builds the program for Linux (x86_64 and aarch64, static, so it also runs in the official Docker image)
   and macOS (Apple silicon), and publishes the release with `SHA2-256SUMS` and `__ID__-0.1.0.zip`. FetchBox downloads
   the program from that release and checks it against `SHA2-256SUMS`. Another platform (macOS on Intel, Windows) is a line
   in the `matrix` of `release.yml`, a line in `assets` and one in `platforms` of `plugin.json`.
4. Users install the ZIP with **Extensions → Install from file**. To list it in the store, add an entry to `plugins/index.json`
   of FetchBox with the address of the ZIP and its SHA-256 (the job summary of the release prints it); see
   `plugins/README.md` in the FetchBox repository.

The installed manifest is pinned to its release (`github.tag` is written into the ZIP's `plugin.json`), so an old
version of the extension never downloads a newer program.

The full guide to what a manifest can do is `plugins/README.md` in the FetchBox repository.
