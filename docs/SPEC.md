# dlmanager — specification

## 1. Goal

A lightweight, self-hosted download manager for a homelab, controlled from a web UI (desktop and mobile). Runs as a single binary / single container on small hardware (mini PC, Raspberry Pi).

## 2. Functional requirements (v1)

- Download list with live progress (status, size, speed, ETA).
- Add download by URL.
- Mobile-friendly UI (mobile-first, installable PWA).
- File category: Photo, Music, Video, Program, Archive, Other. Auto-detected from extension / `Content-Type`, manually changeable. Optional per-category subdirectory.
- Pause and resume (resume must continue from the saved offset, not restart).
- Settings:
  - connections (segments) per download
  - max simultaneous downloads (extra ones are `Queued`)
  - speed limit per download
  - global speed limit
  - changes apply live without restarting running downloads
- Single-user authentication (password or token).

## 3. Future

- BitTorrent: magnet links and `.torrent` files via `librqbit`, behind the same `Downloader` abstraction.

## 4. Architecture

Single process. Backend (Rust) exposes REST + SSE and serves the embedded frontend. No separate frontend deployment.

```
Browser (Svelte SPA/PWA)
   |  REST (commands)   SSE (progress/events)
   v
axum API ──> Scheduler ──> Downloader (HttpDownloader | TorrentDownloader later)
   |             |                  |
   v             v                  v
 Storage      RateLimiter      Filesystem (.part files)
(SQLite)
```

## 5. Directory structure

```
dlmanager/
├── Cargo.toml                  # workspace
├── rust-toolchain.toml
├── crates/
│   ├── core/src/{lib,model,category,scheduler,limiter,error}.rs
│   ├── http_engine/src/{lib,segments,worker}.rs
│   ├── torrent_engine/src/lib.rs        # stub, later librqbit
│   ├── storage/{migrations/,src/{lib,downloads,segments,settings}.rs}
│   ├── api/src/{lib,routes,sse,auth,openapi}.rs
│   └── server/src/main.rs
├── web/
│   ├── src/
│   │   ├── lib/{api/,components/,stores/}
│   │   ├── routes/            # Downloads, AddDownload, Settings
│   │   └── main.ts
│   ├── static/                # manifest.webmanifest, icons
│   ├── package.json
│   └── vite.config.ts
├── scripts/build.sh
├── Dockerfile                 # multi-stage; final image distroless/scratch
├── docker-compose.yml         # volumes: /data (SQLite), /downloads
└── docs/SPEC.md
```

## 6. Key design decisions

### 6.1 Downloader abstraction

```rust
#[async_trait]
pub trait Downloader: Send + Sync {
    async fn start(&self, job: &Job) -> Result<Handle>;
    async fn pause(&self, id: Id) -> Result<()>;
    async fn resume(&self, id: Id) -> Result<()>;
    async fn cancel(&self, id: Id) -> Result<()>;
    fn progress(&self, id: Id) -> Progress;
}
```

DB field `source_kind`: `http` | `magnet` | `torrent_file`.

### 6.2 HTTP engine

- HEAD / ranged GET to learn size, `Accept-Ranges`, `ETag`, `Last-Modified`.
- Split file into N segments (N = connections setting), each fetched with `Range: bytes=a-b`.
- Write to `<name>.part` using positional writes; segment offsets persisted in SQLite (batched, e.g. every 1–2 s).
- Pause = cancel tasks + flush offsets. Resume = restart from stored offsets; verify `ETag`/`Last-Modified` unchanged, else restart.
- No `Accept-Ranges`: single connection, mark download as non-resumable in the UI.
- On completion rename `.part` to final name, handle name collisions.

### 6.3 Scheduler and limits

- `Semaphore` limits concurrent downloads; overflow = `Queued`.
- Two-level token bucket: global and per-download. Every segment acquires tokens before reading from the socket.
- Settings changes are propagated live (watch channel).

### 6.4 Data model (SQLite)

- `downloads`: id, url, filename, category, status, size, downloaded, source_kind, resumable, etag, created_at, finished_at, error
- `segments`: download_id, idx, start, end, downloaded
- `settings`: key/value (connections, max_concurrent, speed_per_download, speed_global)

Statuses: `queued`, `downloading`, `paused`, `completed`, `failed`, `cancelled`.

### 6.5 API (draft)

- `GET /api/downloads`, `POST /api/downloads`
- `POST /api/downloads/:id/pause|resume|cancel`, `DELETE /api/downloads/:id`
- `PATCH /api/downloads/:id` (e.g. change category)
- `GET /api/settings`, `PUT /api/settings`
- `GET /api/events` (SSE: progress, status changes)
- `POST /api/auth/login`, `POST /api/auth/logout`, `GET /api/auth/status`
- Name conflicts on `POST /api/downloads`: the server checks (a) same URL already queued/running/paused/completed,
  (b) file with that name in the download dir, (c) name reserved by an unfinished download. Without `on_conflict`
  it answers `409 {"error":"conflict","conflict":{kind,existing_name,existing_size,existing_modified,existing_id,suggested_name}}`
  with `kind` = `duplicate_url` | `file_exists` | `name_reserved`. The UI shows it and re-sends the request with
  `on_conflict`: `rename` (use `suggested_name`) or `overwrite` (replace the file when the download finishes);
  cancelling means not re-sending. Overwriting a name reserved by an unfinished download is refused (400).
  The engine never overwrites on its own: an unexpected collision at finalize picks the next free name.
- SSE events (`GET /api/events`): `download` (created/status/metadata changed), `progress` (id, downloaded, total, speed),
  `removed`, `settings`, and `resync` (client lagged: reload the list). Data is JSON with a `type` field.
- OpenAPI generated with `utoipa` (derive only, no bundled UI), served at `GET /api/openapi.json` (public) and committed as
  `docs/openapi.json`; `dlmanager --print-openapi` prints it. `utoipa` is an optional feature of `dlm-core`, so the core
  crate stays free of it unless the API enables it. Frontend types are generated with `openapi-typescript` into
  `web/src/lib/api/schema.d.ts` by `scripts/gen-api.sh`; a Rust test and a CI step fail on stale generated files.

### 6.6 Frontend

- Svelte + Vite + TypeScript, mobile-first, list of cards (not a table), large pause/resume buttons, category badge, progress bar, speed/ETA.
- SSE store keeps list in sync; REST for commands.
- PWA: manifest + service worker (app shell cache only).

### 6.7 Security

- Single-user auth, HTTPS via reverse proxy. One shared password from `DLM_PASSWORD` or `--password-file`
  (min. 8 characters); the server refuses to start without one unless `--no-auth` is passed explicitly.
  `POST /api/auth/login` returns a random 256-bit session token, set as an `HttpOnly; SameSite=Strict` cookie
  (needed for SSE) and usable as `Authorization: Bearer`. Public: `/api/health`, `/api/auth/*` (`status` tells the
  UI whether to show the login screen) and the static frontend; everything else answers `401`. Sessions are in
  memory with a 7-day TTL and a cap of 100; failed logins are throttled globally (5 per minute, then `429` with
  `Retry-After`). `Secure` is added to the cookie when `X-Forwarded-Proto: https` is present.
- Sanitize filenames, never write outside download dir.
- Optional blocklist of private IP ranges for URLs (SSRF) when exposed outside LAN.

## 7. Dependencies (Rust)

`tokio`, `axum`, `reqwest` (stream, rustls), `rusqlite` or `sqlx` (SQLite), `serde`, `serde_json`, `tracing`, `tracing-subscriber`, `clap`, `thiserror`, `anyhow`, `async-trait`, `governor` (or custom bucket), `rust-embed`, `mime_guess`, `utoipa`, `tower-http`. Later: `librqbit`.

## 8. Milestones

1. Core: model, SQLite, scheduler, `HttpDownloader` without segments.
2. API + SSE + minimal frontend (list, add).
3. Pause/resume and segmented downloads.
4. Rate limits and settings UI.
5. PWA and mobile polish, auth.
6. BitTorrent via `librqbit`.
