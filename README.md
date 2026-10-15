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

## Frontend

Svelte 5 + Vite + TypeScript in `web/`, mobile-first, no UI framework (about 32 kB gzipped JS). It is embedded into the binary at build time; `cd web && npm run dev` serves it with hot reload and proxies `/api` to `localhost:8080`.

- Download list as cards with live progress (server-sent events): status, size, speed, ETA, category, pause/resume/retry, cancel, remove.
- Sorting (date, name, size, progress, speed, status; either direction), status tabs, category filter and search; the choices are remembered.
- Add by URL with optional name and category; name conflicts open a dialog (keep both / replace / cancel).
- Settings: connections, simultaneous downloads, speed limits (KB/s or MB/s), category folders. Applied live; other open tabs follow.
- Light, dark or follow-the-system theme (no flash on load), login screen, automatic return to it when the session ends.
- English and Polish, chosen from the browser language or in Settings; numbers, dates and name sorting follow the language.

### Adding a language

Translations live in `web/src/lib/i18n/` (no library; a few hundred lines). To add one, say German:

1. Copy `pl.ts` to `de.ts` and translate the values. Keep the `{placeholders}` as they are; a message that depends on a count is an object with the plural forms of that language (`one`, `few`, `many`, `other`, whatever the language needs; see `downloads.cleared`).
2. Add one line to `locales.ts`: `{ code: 'de', name: 'Deutsch', load: async () => (await import('./de')).default }`.

That is all: the language appears in Settings, is loaded on demand as its own small chunk, and is picked automatically when it matches the browser language. `npm run check` fails on a missing or misspelled key, and `npm test` fails on a changed placeholder or a missing plural form. `en.ts` is the source of truth for the keys. Messages coming from the backend (for example the reason a download could not be added) are not translated yet.

`npm run check` (types), `npm test` (vitest, logic only) and `npm run build` run in CI.

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
