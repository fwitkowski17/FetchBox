# FetchBox

Self-hosted download manager web app for a homelab. Single Rust binary serves REST API + SSE + embedded Svelte frontend. BitTorrent is done through `librqbit` (SPEC 6.13).

Full specification: @docs/SPEC.md — read it before making architectural changes.

## Language

The user (Filip) communicates in Polish. Reply in Polish. Code, identifiers, comments, commit messages and docs stay in English.

## Architecture (short)

- One process, one binary. Frontend is built to `web/dist` and embedded via `rust-embed`.
- All download logic (queue, limits, pause/resume) lives in the backend. The browser is only a remote control; downloads must never depend on an open tab.
- Cargo workspace, crates with strict responsibilities:
  - `core` — models, categories, `Downloader` trait, scheduler, rate limiter. No HTTP, no SQL, no axum.
  - `http_engine` — segmented HTTP download with `Range`, pause/resume.
  - `torrent_engine` — adapter over `librqbit` (magnet + `.torrent`), see SPEC 6.13. Torrents are not downloads: own list, table, API and page.
  - `plugins` — extensions (SPEC 6.15): archives, dependencies, commands, the long-running program, the store catalog. No SQL, no axum. `test_plugin` is a stand-in program for its tests; `plugins/` (top level) has the author's guide and the extensions that ship.
  - `storage` — SQLite (`rusqlite` or `sqlx`), migrations, repositories.
  - `api` — axum routes, SSE, auth, OpenAPI (`utoipa`).
  - `server` — binary: wires everything, embeds frontend, CLI (`clap`).
- Frontend: Svelte + Vite + TypeScript in `web/`, mobile-first, PWA. API client/types generated from OpenAPI (`openapi-typescript`).

## Commands

- Backend check: `cargo check --workspace`
- Backend tests: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`
- Frontend dev: `cd web && npm run dev` (Vite proxies `/api` to `127.0.0.1:8080`)
- Frontend check: `cd web && npm run check`
- Full build: `./scripts/build.sh` (web build, then `cargo build --release`)
- Dev backend: `cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads`
- New extension from a template: `cargo new-plugin <id> [-t hook|resolver|process] -o ./.dev/data/plugins` (same as `fetchbox plugin new`)

## Conventions

- Rust: stable toolchain, edition 2021+, no `unwrap()`/`expect()` outside tests and `main` startup. Use `thiserror` in libraries, `anyhow` in the `server` binary. Use `tracing` for logs, never `println!`.
- Keep the hot path (reading from network, writing to disk, rate limiting) free of blocking calls and SQLite access; persist progress periodically/batched via a channel.
- Sanitize file names (path traversal) and validate URLs. Never write outside the configured download dir.
- Frontend: TypeScript strict, no `any`. Svelte components small, stores in `src/lib/stores`. No heavy UI frameworks; plain CSS or Tailwind.
- Do not hand-edit generated API types in `web/src/lib/api/`; regenerate them.
- Prefer small, reviewable commits. Add tests with new core logic (scheduler, limiter, category detection, segment planning).

## Feature scope

v1: download list, mobile support, file categories (Photo, Music, Video, Program, Archive, Other), pause/resume, settings (connections per download, max concurrent downloads, default speed per download and total speed limit; a single download can get its own speed from its menu, which replaces the default).
BitTorrent (magnet + .torrent) lives on its own page; see SPEC 6.13 for what it leaves out.

## Do not

- Do not add Node.js or any JS runtime to the production image.
- Do not introduce a second deployable service.
- Do not add dependencies without a short justification (target: lightweight runtime).
