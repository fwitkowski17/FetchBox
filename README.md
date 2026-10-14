# dlmanager

Self-hosted download manager for a homelab. One Rust binary serves the REST API, SSE and the embedded Svelte frontend. See [docs/SPEC.md](docs/SPEC.md).

Status: backend in progress (download engine, REST API, SSE and auth work; frontend not started).

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

## Authentication

The API is protected by a single shared password. The server **refuses to start** without one:

```bash
DLM_PASSWORD='at-least-8-chars' cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads
# or: --password-file /run/secrets/dlm_password
# or, only on a fully trusted network: --no-auth
```

Log in with `POST /api/auth/login {"password": "..."}`: the response sets an `HttpOnly` session cookie and also returns a token usable as `Authorization: Bearer <token>`. After 5 failed attempts logins are locked for up to a minute. Sessions are kept in memory (a restart logs everyone out). Serve it over HTTPS via a reverse proxy when exposed beyond the LAN (send `X-Forwarded-Proto: https` so the cookie gets `Secure`).

## Build

```bash
./scripts/build.sh          # web build, then cargo build --release -> target/release/dlmanager
docker compose up --build   # UI on http://localhost:8080, volumes ./data and ./downloads
```

## Layout

Workspace crates live in `crates/` (`core`, `http_engine`, `torrent_engine`, `storage`, `api`, `server`); package names are prefixed `dlm-` to avoid clashing with std `core`. The frontend is in `web/`.

## Dependency notes

`tokio` + `axum` (one async stack), `rust-embed` + `mime_guess` (embedded SPA), `rusqlite` bundled (lighter than sqlx, no system SQLite), `clap` (CLI), `serde`/`serde_json`, `thiserror`/`anyhow`, `tracing`, `async-trait` (`Downloader` trait object).
