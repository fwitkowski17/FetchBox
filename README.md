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

## Options worth knowing

- `--block-private-urls`: refuse URLs that point at loopback, private, link-local or similar addresses (SSRF protection). Off by default so you can download from a NAS or another host in your LAN; turn it on if the manager is reachable from outside.
- Settings (in the UI/API): connections per download, max simultaneous downloads, per-download and global speed limits, and sorting downloads into per-category subdirectories. All apply live.

## API documentation and types

The HTTP API is described by an OpenAPI 3.1 document generated from the Rust code (`utoipa`). It is served at `/api/openapi.json` (public) and committed as [docs/openapi.json](docs/openapi.json). The frontend types in `web/src/lib/api/schema.d.ts` are generated from it and must not be edited by hand.

After changing anything in the HTTP API run:

```bash
./scripts/gen-api.sh   # rewrites docs/openapi.json and web/src/lib/api/schema.d.ts
```

A test fails when `docs/openapi.json` is stale, and CI fails when the generated TypeScript is. `dlmanager --print-openapi` prints the document without starting the server.

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
