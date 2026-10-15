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
  - sort downloads into a subdirectory per category (`category_subdirs`, off by default)
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
- Disk space: files are pre-sized with `set_len` (sparse on ext4, xfs, btrfs, apfs, tmpfs), so free space only shrinks
  as bytes are written. Room is reserved virtually: as soon as the size is known (the probe of `POST /api/downloads`)
  the rest of the file counts against the free space of its disk. The reservation of a download is what it still
  has to write (size minus what it has stored), held while it is queued, running or paused and given back when it
  completes (the bytes are on the disk by then), fails, is cancelled or removed. A new download is refused (`507`) when
  it does not fit into `available - reserved by the others`, so five 50 GB files cannot all be queued on a 100 GB disk.
  Reservations are kept per file system (device id): a category subdirectory can be a mount point on another disk, and
  a path that does not exist yet is judged by its nearest existing parent. Resuming a *failed* download takes its
  reservation again and is refused the same way if the space went elsewhere meanwhile. As a safety net the engine
  checks once more when a download actually starts (the disk may have been filled by something else, or the size was
  unknown at add time): the rest of the file plus what the other running downloads on that disk still have to write
  must fit, otherwise it fails with `not enough disk space: X needed, Y available` before anything is created. No
  safety margin is kept: the data directory may be on another disk, and the download directory is the user's to fill.
  Unknown sizes (no `Content-Length`) cannot be reserved or checked, and a platform without `statvfs` skips the checks
  with a warning instead of blocking downloads. Only downloads that probe at the very same moment as the engine check
  can race; the loser fails on the write error like any full disk.

### 6.3 Scheduler and limits

- `Semaphore` limits concurrent downloads; overflow = `Queued`.
- Two-level token bucket: global and per-download. Every segment acquires tokens before reading from the socket.
- Settings changes are propagated live (watch channel).

### 6.3.1 Categories and directories

- Category = file extension first (servers often send `application/octet-stream`), then `Content-Type`
  (`image/*` Photo, `audio/*` Music, `video/*` Video, archive and installer MIME types), else `Other`.
  An explicit `category` in the add request wins; it can be changed later with `PATCH`.
- With `category_subdirs` on, a new download goes to `<download dir>/<category>/` (`photo`, `music`, `video`,
  `program`, `archive`, `other`). The subdirectory is stored per download (`downloads.subdir`) when it is added:
  changing the setting or the category afterwards never moves an existing file. The `.part` file lives next to the
  final file; conflict checks and name reservation are per subdirectory.

### 6.4 Data model (SQLite)

- `downloads`: id, url, filename, subdir, category, status, size, downloaded, source_kind, resumable, etag, overwrite, created_at, finished_at, error
- `segments`: download_id, idx, start, end, downloaded
- `settings`: key/value (connections, max_concurrent, speed_per_download, speed_global, category_subdirs)

Statuses: `queued`, `downloading`, `paused`, `completed`, `failed`, `cancelled`.

### 6.5 API (draft)

- `GET /api/downloads`, `POST /api/downloads`
- `POST /api/downloads/:id/pause|resume|cancel`, `DELETE /api/downloads/:id`
- `PATCH /api/downloads/:id` (e.g. change category)
- `GET /api/settings`, `PUT /api/settings`
- `GET /api/disk` (free space of the download disk)
- `GET /api/events` (SSE: progress, status changes)
- `POST /api/auth/login`, `POST /api/auth/logout`, `GET /api/auth/status`, `POST /api/auth/setup`, `POST /api/auth/password`
- Name conflicts on `POST /api/downloads`: the server checks (a) same URL already queued/running/paused/completed,
  (b) file with that name in the download dir, (c) name reserved by an unfinished download. Without `on_conflict`
  it answers `409 {"error":"conflict","conflict":{kind,existing_name,existing_size,existing_modified,existing_id,suggested_name}}`
  with `kind` = `duplicate_url` | `file_exists` | `name_reserved`. The UI shows it and re-sends the request with
  `on_conflict`: `rename` (use `suggested_name`) or `overwrite` (replace the file when the download finishes);
  cancelling means not re-sending. Overwriting a name reserved by an unfinished download is refused (400).
  The engine never overwrites on its own: an unexpected collision at finalize picks the next free name.
- `GET /api/disk` answers `{total, available, reserved}` for the file system of the download directory: `total` is used
  plus available (blocks reserved for root are not counted, so the share matches `df`'s `Use%`), `reserved` is what
  queued, running and paused downloads on that file system with a known size still have to write (see 6.2).
  `POST /api/downloads` answers `507 {"error":"insufficient storage","needed","available"}` when the file does not fit
  next to the other reservations: `needed` is the file size, `available` the free space minus what others have
  reserved; nothing is stored then.
- SSE events (`GET /api/events`): `download` (created/status/metadata changed), `progress` (id, downloaded, total, speed),
  `removed`, `settings`, and `resync` (client lagged: reload the list). Data is JSON with a `type` field.
- OpenAPI generated with `utoipa` (derive only, no bundled UI), served at `GET /api/openapi.json` (public) and committed as
  `docs/openapi.json`; `dlmanager --print-openapi` prints it. `utoipa` is an optional feature of `dlm-core`, so the core
  crate stays free of it unless the API enables it. Frontend types are generated with `openapi-typescript` into
  `web/src/lib/api/schema.d.ts` by `scripts/gen-api.sh`; a Rust test and a CI step fail on stale generated files.

### 6.6 Frontend

- Svelte + Vite + TypeScript, mobile-first, list of cards (not a table), large pause/resume buttons, category badge, progress bar, speed/ETA.
- Styling: Tailwind CSS 4 at build time only. Theme colours are CSS variables per theme (`app.css`) mapped into Tailwind with
  `@theme inline`, so utilities follow light / dark / black without `dark:` variants. Font: Inter variable, self-hosted
  (`latin` and `latin-ext` subsets only; nothing from a CDN). Tests use `data-*` attributes and ARIA roles, never style classes.
- Accent colour: the user picks one of a fixed set (`util/accent.ts`: id, OKLCH hue, saturation factor). The theme owns lightness and
  saturation (`--accent-l`, `--accent-c`, text colour on the accent, tint strength), the choice only sets `--accent-h` /
  `--accent-k`; `--accent` is `oklch(...)` and `--accent-soft` a `color-mix` with the card colour, so every accent works in
  light, dark and black. The choice is stored as `{id, hue, chroma}` so `index.html` can apply it before first paint without
  the table. A test derives the contrast of every accent in every theme from the CSS and requires WCAG AA (4.5:1 for text on the
  accent and accent text on cards, 4.2:1 on the tint). Status filters carry icons coloured by state.
- Disk ring: at the bottom of the side menu (left bottom corner, also in the drawer) a ring shows the used share of the
  download disk, a lighter arc the space reserved for unfinished downloads, "N free of M" next to it, the reserved amount and, when
  the reservations exceed the free space (something else filled the disk), how much is missing. It shows the disk of
  the download directory; a category subdirectory on another disk is covered by the checks but not drawn. Accent colour up to 85 % used, amber from 85 %, red from 95 % or when the queue does
  not fit. Polled every 10 s (not while the tab is hidden) and refreshed when a download finishes; hidden if the server
  cannot tell. The add form shows the `507` as "needs X, only Y free after the space reserved for other downloads".
- Layout: side menu. From 64rem a permanent left panel (navigation, status and category filters with counts, speed, connection
  state, theme switch); below that the same panel is a drawer behind a top-bar button (`inert` page behind it, Esc and the
  backdrop close it, focus returns to the button) plus a floating add button on the list and tags for the applied filters.
  Choosing a filter on another page returns to the list. Forms use a narrow column; the card list is a responsive grid.
- First-run wizard (`routes/Setup.svelte`), shown while the server reports `setup_required` and until the user finishes
  it: look (language, theme, accent; applied live, stored like in Settings), account (setup code, password, repeat),
  download defaults (the same fields as Settings, optional), done. The steps after the account one run signed in;
  reloading in between simply opens the app. Settings has a "change password" card for UI-created accounts and an
  explanation for an external password.
- SSE store keeps list in sync; REST for commands. Command responses are not patched into the list (they can be older
  than the event that followed); the stream is the single source of truth, and the list is re-fetched on every
  (re)connect and on `resync`.
- Sorting (date added, name, size, progress, speed, status; unknown sizes always last), status tabs, category filter,
  search; remembered in `localStorage`. System / light / dark / black theme via CSS tokens (`[data-theme]` plus
  `prefers-color-scheme`; black is dark with a pure black background for OLED), applied before first paint.
- Settings form works in KB/MB and "limit on/off"; the API stays in bytes/s with `0` = unlimited. The form follows
  server changes made by other clients unless the user has unsaved edits.
- i18n without a library: `web/src/lib/i18n/` holds one typed dictionary per language (`en.ts` is the source of truth for
  the keys, other languages must provide all of them), `{name}` placeholders, plural forms via `Intl.PluralRules`
  (Polish needs one/few/many/other), and a registry (`locales.ts`) where each language is one line and a lazily loaded
  chunk. The language comes from the stored choice or `navigator.languages` (falling back to English); numbers, dates and
  name sorting use `Intl` for the active language; substituted values (file names) are rendered as text, never as HTML.
  Tests check every language for the same keys and placeholders and for the plural forms it needs. Backend messages
  (`error` strings) are shown as sent and are not translated.
- PWA: manifest + service worker (app shell cache only).

### 6.7 Security

- Single-user auth, HTTPS via reverse proxy. One shared password, from the first source that applies:
  `DLM_PASSWORD` / `--password-file` (external: wins, cannot be changed in the UI), the account created in the
  first-run setup (below), or none at all with an explicit `--no-auth`. Without any of them the server still starts
  but stays locked until setup is done, so a forgotten password never exposes it. Passwords have 8 to 256 characters.
  `POST /api/auth/login` returns a random 256-bit session token, set as an `HttpOnly; SameSite=Strict` cookie
  (needed for SSE) and usable as `Authorization: Bearer`. Public: `/api/health`, `/api/auth/*` (`status` tells the
  UI whether to show the login screen) and the static frontend; everything else answers `401`. Sessions are in
  memory with a 7-day TTL and a cap of 100; failed logins are throttled globally (5 per minute, then `429` with
  `Retry-After`). `Secure` is added to the cookie when `X-Forwarded-Proto: https` is present.
- First-run setup: while no account exists `GET /api/auth/status` reports `setup_required` and everything else
  answers `401`. The server logs a one-time setup code (8 characters from an unambiguous alphabet, regenerated on
  every start, never stored). `POST /api/auth/setup {code, password}` creates the account and logs in; wrong codes
  (`403`) share the login throttle (5 per minute, then `429`), the code is compared in constant time, and the
  endpoint answers `409` once an account exists or an external password is configured, so it cannot be used to take
  over. The password is stored as an Argon2id hash (random salt, PHC string) under the `password_hash` key of the
  `settings` table; it is not part of `Settings` and never leaves the database. `POST /api/auth/password
  {current_password, new_password}` (logged in) changes it (`403` wrong current password, `409` for an external
  password) and ends every other session. Hashing runs on the blocking thread pool.
- Sanitize filenames, never write outside download dir.
- Optional blocklist of private IP ranges for URLs (SSRF), `--block-private-urls`, off by default because fetching
  from a NAS in the LAN is normal use. It blocks loopback, RFC 1918, link-local (cloud metadata), CGNAT, multicast,
  unspecified and IPv6 unique-local/link-local addresses (IPv4-mapped IPv6 is judged by its IPv4 part), and is
  enforced in three places so it cannot be bypassed: IP literals in the URL, a filtering DNS resolver for host
  names (also stops DNS rebinding), and every redirect hop. Violations are `400` (literal) or `502` (name).
- Crash safety: progress is persisted once per tick; after a kill the interrupted downloads are re-queued and resume
  from the saved segment offsets (verified by a test that kills a runtime mid-download).

## 7. Dependencies (Rust)

`tokio`, `axum`, `reqwest` (stream, rustls), `rusqlite` or `sqlx` (SQLite), `serde`, `serde_json`, `tracing`, `tracing-subscriber`, `clap`, `thiserror`, `anyhow`, `async-trait`, `governor` (or custom bucket), `rust-embed`, `mime_guess`, `utoipa`, `tower-http`, `argon2` (password hash), `rustix` (free disk space via `statvfs`, no `unsafe`; Unix). Later: `librqbit`.

## 8. Milestones

1. Core: model, SQLite, scheduler, `HttpDownloader` without segments.
2. API + SSE + minimal frontend (list, add).
3. Pause/resume and segmented downloads.
4. Rate limits and settings UI.
5. PWA and mobile polish, auth.
6. BitTorrent via `librqbit`.
