# Bootstrap prompt for Claude Code

Run `claude` in the repo root (with `CLAUDE.md` and `docs/SPEC.md` present) and paste the prompt below.

---

Read CLAUDE.md and docs/SPEC.md. Create the repository skeleton exactly as described in section 5 of the spec. This is milestone 0: structure only, no real download logic.

Requirements:

1. Cargo workspace with crates: `core`, `http_engine`, `torrent_engine` (empty stub), `storage`, `api`, `server`. Each crate compiles, has a `lib.rs` (or `main.rs` for `server`) and the module files listed in the spec, with minimal placeholder types (`Download`, `Status`, `Category`, `Settings`, the `Downloader` trait) and doc comments, no `todo!()` panics on startup paths.
2. `server` starts an axum app on port 8080 with `GET /api/health` returning `{"status":"ok"}`, serves the embedded `web/dist` (use `rust-embed`, with a fallback to `index.html` for SPA routes), and parses `--data-dir`, `--download-dir`, `--port` via clap.
3. `storage` has an initial SQLite migration creating `downloads`, `segments`, `settings` as in section 6.4, and a function that opens the DB and runs migrations at startup.
4. `core::category` implements extension-based detection for Photo, Music, Video, Program, Archive, Other, with unit tests.
5. `web/` is a Svelte + Vite + TypeScript project with strict TS, routes/pages for Downloads, AddDownload, Settings (empty but navigable, mobile-first layout), a Vite proxy for `/api`, and a PWA manifest in `static/`.
6. Add `scripts/build.sh`, multi-stage `Dockerfile` (node build stage, rust build stage, distroless final image), `docker-compose.yml` with `/data` and `/downloads` volumes, `.gitignore`, `rust-toolchain.toml`, `rustfmt.toml`, and a short `README.md`.
7. Add a GitHub Actions workflow running `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and `npm run check` + `npm run build` in `web/`.

Process:
- First show me a short plan (file tree), wait for my OK, then implement.
- After implementing, run `cargo check --workspace`, `cargo test --workspace`, and the frontend build, and fix what fails.
- Initialize git and make one commit: `chore: repository skeleton`.
- Do not implement download logic, auth, or the torrent engine yet.
