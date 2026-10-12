# dlmanager

Self-hosted download manager for a homelab. One Rust binary serves the REST API, SSE and the embedded Svelte frontend. See [docs/SPEC.md](docs/SPEC.md).

Status: milestone 0 (repository skeleton, no download logic yet).

## Develop

```bash
cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads
cd web && npm install && npm run dev   # Vite proxies /api to localhost:8080
```

Checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && npm run check
```

## Build

```bash
./scripts/build.sh          # web build, then cargo build --release -> target/release/dlmanager
docker compose up --build   # UI on http://localhost:8080, volumes ./data and ./downloads
```

## Layout

Workspace crates live in `crates/` (`core`, `http_engine`, `torrent_engine`, `storage`, `api`, `server`); package names are prefixed `dlm-` to avoid clashing with std `core`. The frontend is in `web/`.

## Dependency notes

`tokio` + `axum` (one async stack), `rust-embed` + `mime_guess` (embedded SPA), `rusqlite` bundled (lighter than sqlx, no system SQLite), `clap` (CLI), `serde`/`serde_json`, `thiserror`/`anyhow`, `tracing`, `async-trait` (`Downloader` trait object).
