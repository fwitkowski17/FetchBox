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
│   │   ├── routes/            # Downloads, Settings (+ Setup wizard); adding is a dialog
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
  as bytes are written. What a *running* download still has to write (size minus what it has stored) is reserved: it
  counts against the free space of its disk until the download completes (the bytes are on the disk by then), pauses,
  fails or is cancelled. "Running" means holding a download slot, whether or not the engine has reported its start yet,
  so two downloads added in quick succession see each other. Queued downloads waiting for a slot, paused ones and held
  ones (below) reserve nothing: they are not writing anything, and a forgotten paused entry must not block the disk.
  A new download is refused (`507`) when it does not fit into `available - reserved`. Two cases are told apart.
  *Virtually* short (fits into the free space, but not next to what the running downloads still have to write): the
  request is refused with `507` and the user may repeat it with `ignore_space`. *Really* short (larger than the space
  that is free right now): the entry is added but not started; it is stored as `paused` with `waiting_space` set, nothing
  is written for it, and resuming it is refused (`507`, state unchanged) until the free space covers what is left.
  Leaving `paused` ends the hold. The virtual refusal is advisory: the same request with `ignore_space: true` is accepted
  ("add anyway", the user's call: files may be moved or deleted before the space is needed). Such a download is flagged in
  the database (`downloads.ignore_space`) and the engine skips its own free-space check for it, also after a restart or a
  resume; if the disk really fills up it fails with the write error. The ring then shows the over-commitment.
  Several big files can thus be queued on a small disk; each is checked when it gets its slot (see the engine check below).
  Reservations are kept per file system (device id): a category subdirectory can be a mount point on another disk, and
  a path that does not exist yet is judged by its nearest existing parent. Resuming (paused or failed) checks only the
  space that is really free (not what the running downloads still have to write): if what is left does not fit, the
  request is refused with `507` and nothing changes, unless the download was added with `ignore_space`. As a safety net
  the engine
  checks once more when a download actually starts (the disk may have been filled by something else, or the size was
  unknown at add time): the rest of the file plus what the other running downloads on that disk still have to write
  must fit, otherwise it fails with `not enough disk space: X needed, Y free (Z of it still to be written by running downloads)`
  before anything is created. No safety margin is kept: the data directory may be on another disk, and the download
  directory is the user's to fill.
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

- `downloads`: id, url, filename, subdir, category, status, size, downloaded, source_kind, resumable, etag, overwrite, ignore_space, waiting_space, archived, created_at, finished_at, error
- `segments`: download_id, idx, start, end, downloaded
- `settings`: key/value (connections, max_concurrent, speed_per_download, speed_global, category_subdirs)

Statuses: `queued`, `downloading`, `paused`, `completed`, `failed`, `cancelled`.

### 6.5 API (draft)

- `GET /api/downloads`, `POST /api/downloads`
- `POST /api/downloads/:id/pause|resume|cancel`, `DELETE /api/downloads/:id[?delete_file=true]` (removes the entry; with
  `delete_file` also the finished file, only for a completed download and only the plain file name the engine finally
  chose, inside the download's own directory; a file that is already gone is fine, any other failure keeps the entry)
- `PATCH /api/downloads/:id` (`category` and/or `archived`: archiving hides a finished, failed or cancelled download from
  the main list, `409` for an unfinished one; the file stays where it is and the entry still counts for duplicate and name
  checks)
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
  the running downloads on that file system with a known size still have to write (see 6.2).
  `POST /api/downloads` answers `507 {"error":"insufficient storage","needed","free","reserved"}` when the file does not
  fit next to what the running downloads still have to write (`needed > free - reserved`) while it still fits into the free space; nothing is
  stored then. Sending the request again with `ignore_space: true` adds it anyway. A file larger than the free space is
  answered with `201` and a download that is `paused` with `waiting_space: true`; `POST .../resume` on it answers `507`
  until there is room.
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
  download disk, a lighter arc the space the running downloads still have to write, "N free of M" next to it, that amount and, when
  it exceeds the free space (something else filled the disk), how much is missing. It shows the disk of
  the download directory; a category subdirectory on another disk is covered by the checks but not drawn. Accent colour up to 85 % used, amber from 85 %, red from 95 % or when the queue does
  not fit. Polled every 10 s (not while the tab is hidden) and refreshed when a download finishes; hidden if the server
  cannot tell. The add form turns the `507` into a dialog: what the file needs, what is free, how much of it the running downloads
  still have to write and what is left, with "Add anyway" (resends with `ignore_space`) and "Cancel"; a conflict dialog after it does not ask again.
- Held download: a card with `waiting_space` shows the badge "Waiting for space" and, instead of the progress bar, a gauge:
  one bar that is the whole need, filled with what the free space covers (follows the disk figures, so it fills while the
  user frees space), the missing part striped, with "Free / Needs / Missing" below. Once the free space covers the need the
  text says so and the bar turns green; the user resumes by hand. Adding such a file shows a warning toast, not a dialog.
  Pressing "Resume" while the disk is still too full (`507`, also for a failed download) opens a dialog with the same gauge
  instead of a toast: it follows the disk figures, "Try again" retries, and once there is room the button reads "Resume" and
  starts the download (the dialog closes when the resume works or the download is gone).
- Adding from the list: with at least one download on the list the page header has a primary "Add download" button at its
  right end (next to "Clear finished"); an empty list has the big button in the middle instead, and below 64rem the floating
  button takes over (the header one is hidden there, so there is one add button per layout). The side menu has no "Add"
  item: it lists the pages and the filters. Toasts sit at the bottom right on wide screens (so they never cover the header
  buttons) and under the top bar on narrow ones (the floating button is at the bottom there).
- Selecting several downloads: a "Select" button in the list header switches the list to selection mode. Cards get a checkbox
  and become click targets (their own buttons are hidden meanwhile), the header buttons step aside and an action bar appears
  at the bottom (next to the side panel on wide screens; on a phone the count on top and four evenly spread buttons below).
  The bar has "Select all" (everything visible, toggling back to "Clear selection"), "Category", "Archive" / "Restore",
  "Delete" and "Done"; Escape also leaves the mode (unless a dialog is open) as does leaving the page. Only downloads that are
  on screen can be selected: a filter or search that hides a selected card deselects it. All actions are the per-download
  requests sent in parallel, with one summary toast (and the first error when some failed). "Category" opens a dialog with the
  six categories and only sets the category (the files stay where they are). "Archive" takes the finished, failed and
  cancelled ones among the selected and says how many unfinished ones it left alone; when only archived ones are selected
  the same button reads "Restore". "Delete" opens the delete dialog for all of them: the count, a note about the unfinished
  ones (stopped, partial files deleted) and, when completed ones are included, "Also delete the N downloaded files" (off by
  default). The cards' menu also has "Archive" / "Restore from archive".
- Archived: a status tab of its own ("Archived", in the side panel). "All" and the other status tabs show only what is not
  archived, so archiving really hides an entry; "Clear finished" leaves archived entries alone; a list where everything is
  archived says so and offers to show them.
- Card menu: the card's action row is the primary button (pause / resume / retry), a "more" button and, as before, cancel
  (unfinished) or "remove from list, the file stays" (finished). The "more" menu (replacing the old category select) has
  "Copy link" (the clipboard API where the page is a secure context, a selection fallback on plain-HTTP LAN pages),
  "Change category" (a second level with the current one checked) and "Delete...". It opens towards the side that has room
  (up or down, left- or right-aligned), closes on Escape / outside click / Tab, and works with the arrow keys, Home and End.
  "Delete..." opens a dialog (`stores/deleteDialog`): for a finished download it offers "Also delete the file from the disk"
  (off by default, with a warning when ticked, and a button that then reads "Delete with file"); for an unfinished one it
  says that the download is stopped and the partial file goes too, and has no such option.
- Adding a download is a modal dialog (`AddDownloadDialog`), not a page: opened from the list header, the
  floating button, the empty list and the end of the setup wizard (all through `stores/addDialog`), mounted once in the app
  shell. The URL field is focused, Enter submits, Escape / Cancel / the backdrop close it and keep what was typed (a
  success clears it), the name-conflict and "does not fit" dialogs open on top of it, and a success closes everything and
  shows the list. Old `#/add` links still work: they open the dialog and the hash goes back to `#/`.
- Motion: short and functional (about 150-250 ms), never decorative loops. Dialogs fade and rise in and fade and sink out
  (the native `<dialog>` is closed only after the exit has played; reopening during the exit cancels it); toasts fly in,
  fade out and the rest glide into place; the drawer scrim fades; the floating button pops in; a card added to an existing
  list rises in (the first card of an empty list and the cards of a freshly opened page do not animate, and the list is
  never re-ordered with animation because sorting by speed would make it jump every second); pages fade in on navigation;
  progress bars, the space gauge and the disk ring glide to new values; buttons give a small press feedback. Everything
  respects `prefers-reduced-motion`: CSS animations and transitions are cut to nothing in `app.css`, Svelte transitions take
  their duration from `util/motion.ts`, which is 0 then.
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
