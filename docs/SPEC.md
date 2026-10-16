# FetchBox — specification

## 1. Goal

A lightweight, self-hosted download manager for a homelab, controlled from a web UI (desktop and mobile). Runs as a single binary / single container on small hardware (mini PC, Raspberry Pi).

## 2. Functional requirements (v1)

- Download list with live progress (status, size, speed, ETA).
- Add download by URL: `http(s)://`, `ftp://`, `ftps://` and `sftp://` (6.12), with a login where the source needs one.
- Mobile-friendly UI (mobile-first, installable PWA).
- File category: Photo, Music, Video, Program, Archive, Other. Auto-detected from extension / `Content-Type`, manually changeable. Optional per-category subdirectory.
- Pause and resume (resume must continue from the saved offset, not restart).
- Checksum verification: a download can carry an expected md5, sha1, sha256 or sha512 digest; the finished file is checked against
  it (see 6.2.2).
- Settings:
  - connections (segments) per download
  - max simultaneous downloads (extra ones are `Queued`)
  - default speed of each download (a single download can get its own speed from its menu, which replaces the default, see 6.6)
  - total speed limit of all downloads together
  - sort downloads into a subdirectory per category (`category_subdirs`, off by default)
  - download hours (`schedule_*`, off by default): download only between two times of the day, see 6.3.2
  - check for updates (`check_updates`, on by default): whether a newer release exists, shown in Settings (see 6.9); also announce
    pre-releases (`check_prereleases`, off by default)
  - changes apply live without restarting running downloads
- Single-user authentication (password or token).

## 3. Future

- BitTorrent: magnet links and `.torrent` files via `librqbit`, behind the same `Downloader` abstraction.
- More sources behind the same abstraction: WebDAV and S3-compatible storage (S3, R2, MinIO; a pre-signed link is already an HTTPS
  address). FTP, FTPS and SFTP exist (6.12); still missing there are SFTP with a private key and FTPS with a self-signed certificate.
- More ways to authorize a download: tokens and custom headers, and changing the login of an existing download (a user name and
  password already work for HTTP basic authentication, FTP, FTPS and SFTP, see 6.12).

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
FetchBox/
├── Cargo.toml                  # workspace
├── rust-toolchain.toml
├── crates/
│   ├── core/src/{lib,model,category,scheduler,schedule,limiter,stats,error}.rs
│   ├── http_engine/src/{lib,segments,worker,source,ftp,sftp,hostkeys,net,listing,probe}.rs   # also FTP(S) and SFTP, see 6.12
│   ├── torrent_engine/src/lib.rs        # stub, later librqbit
│   ├── storage/{migrations/,src/{lib,downloads,known_hosts,segments,settings,stats}.rs}
│   ├── api/src/{lib,routes,sse,auth,openapi}.rs
│   └── server/src/main.rs
├── web/
│   ├── src/
│   │   ├── lib/{api/,components/,stores/}
│   │   ├── routes/            # Downloads, Statistics, Settings (+ Setup wizard); adding is a dialog
│   │   └── main.ts
│   ├── static/                # manifest.webmanifest, icons
│   ├── package.json
│   └── vite.config.ts
├── scripts/build.sh
├── deploy/                    # install.sh (Linux host, systemd) and proxmox/ (LXC installer)
├── packaging/                 # DEB/RPM: nfpm.yaml, unit, env file, maintainer scripts, build.sh, smoke-test.sh
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

DB field `source_kind`: `http` | `ftp` (also `ftps://`) | `sftp` | `magnet` | `torrent_file`. The engine of 6.2 (`HttpDownloader`, the name kept from
the first version) is the one implementation of the trait for `http`, `ftp` and `sftp`: the scheme of the address picks the transport (6.12).

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

- Write path: a segment reads the next chunk from the socket only after the previous one has been written, so a disk that is
  slower than the line slows the transfer through TCP and memory stays bounded (a chunk plus tokio's file buffer, at most about
  2 MiB, per connection). A segment counts a chunk only after the write has completed (`flush` waits for tokio's blocking write),
  so a counter never runs ahead of what the OS has accepted and a killed process loses nothing it counted. Data is in the OS page
  cache until the engine forces it out: every `CHECKPOINT` (2 s, started at the first tick, one at a time, on the blocking pool so
  the transfer goes on) it snapshots the offsets, `fdatasync`s the `.part` file and only then makes the snapshot the *durable*
  offsets. `Event::Progress` carries the live `downloaded` (for the screen) and the durable `segments`; the manager stores the
  durable total and offsets, so what is stored for resuming is always on the disk. `pause` stops the tasks, syncs and reports the
  synced snapshot in `Paused` (a resume or start is refused while it syncs: `pausing`); a failed download syncs before its last
  `Progress`; a failed sync keeps the earlier durable offsets (or fails the download during a checkpoint). The finished file is
  synced before the final rename and the directory after it (best effort). A plain stream cannot be resumed and is not synced
  during the transfer. The progress and speed shown are what was received, which can exceed the disk's sustained rate while the
  cache fills. Not yet measured on slow (SMR) disks.

### 6.2.2 Checksum verification

- **Adding**: `AddRequest.checksum {algo, value}` (`algo`: `md5` | `sha1` | `sha256` | `sha512`). The value is what the user pasted and
  is normalised by `fetchbox_core::checksum::normalize`: lowercase hexadecimal of the length of the algorithm (32, 40, 64, 128).
  Forgiving: surrounding space, a label (`sha256:`, `SHA256 =`, `sha256sum:`), `0x`, separators inside the digest (`d4:1d:...`,
  groups of eight) and a line of a checksum file (`<digest>  <name>`, the first word) are understood, only the first line counts.
  Anything that is not a digest of that algorithm is `400` and nothing is added. A listing of several files has no checksum (each
  file has its own).
- **Status `verifying`** (between `downloading` and `completed`): when the engine reports a download with a checksum finished,
  the manager releases its download slot at once (reading a file back is not a transfer; the next download starts), sets
  `verifying` and hashes the final file on the blocking pool (1 MiB reads, `fetchbox_http_engine::verify`; `sha2`, `sha1` and
  `md-5`, which use the CPU's SHA instructions where present). While it runs, `progress` events carry the bytes read in
  `downloaded` (and the rate in `speed`), and the list shows them in place of `downloaded` (the stored value stays the size).
  `finished_at` is not set yet and the download does not count as completed in the statistics.
- **Match**: `completed`, `verified: true`. **Mismatch** or a file that cannot be read: `failed`, `verify_failed: true`, the
  `error` says why and `checksum.actual` holds the digest the file really has (empty when it could not be read). The failure is
  counted as a failure in the statistics; keeping the file later replaces that row by a completed one.
- **What the user can do with a failed verification** (the file stays on disk until they decide): *download again*
  (`POST .../resume`: the file is deleted, offsets and etag are reset and the download starts from the beginning, with the same
  checksum), *keep* (`POST /api/downloads/:id/keep`: `completed`, `verified: false`; `verify_failed` and `checksum.actual` stay, so the list can show that it was kept although the checksum did not match) or *delete with the
  file* (`DELETE ...?delete_file=true`, which is allowed for a completed download, one being verified, and one that failed its
  verification). `keep` also skips a verification that is still running (no verdict is left then; `409` for any other download). `cancel` on a download
  being verified is refused (`409`): the file is complete; `DELETE` stops the verification (and may take the file).
- **Restart**: a download left `verifying` by a dead process hashes its file again at the next start.
- **Data**: `downloads.checksum_algo`, `checksum_expected`, `checksum_actual`, `verified`, `verify_failed`. Adding the status
  needed a table rebuild (SQLite cannot change a CHECK constraint): migration 0010 copies the table, carries the `AUTOINCREMENT`
  counter over and recreates the history trigger; the migration runner switches foreign keys off around migrations and checks
  them afterwards.

### 6.2.1 Directory index pages

- A URL that answers with an HTML page is read once more (at most 2 MiB, through the same client, so the private-address
  policy and the redirect checks apply) and, if it is a *directory index* ("Index of ..." or "Directory listing for ..." in
  the title or first heading: Apache, nginx, lighttpd, Python's `http.server`), reported by `POST /api/probe` as
  `kind: "listing"`. Any other page is a file like before. No HTML parser: the anchors of the page are scanned.
- Only the direct children of the page are listed: folders (`is_dir`, address ending in `/`) first, then files, each group in
  page order, with the decoded name, absolute address, the category of the file name and a size when the page shows one
  (bytes as nginx prints them, or `1.5G` as Apache does; none for Python's server). Links to other hosts or other places, the
  parent folder, sort links (`?C=N;O=D`), non-`http(s)` schemes and names with `/`, `\` or control characters are dropped.
  At most 2000 items are returned (`truncated`). Folders are not scanned recursively: the user opens one, which is a new probe.
- A listing is not a download by itself: the chosen files are added one by one with the ordinary `POST /api/downloads`
  (the server names them, detects the category and checks space for each).

### 6.3 Scheduler and limits

- `Semaphore` limits concurrent downloads; overflow = `Queued`.
- Two-level token bucket: total and per-download. Every segment acquires tokens before reading from the socket. Three
  limits, in this order: (1) the download's own speed (`downloads.speed_limit`, bytes/s, `0` = none set), else (2) the
  default speed of each download (`speed_per_download`, `0` = none), which sets the rate of the per-download bucket; (3) the
  total limit (`speed_global`), the shared bucket that caps the sum of all downloads whatever their own rates are. An own
  speed replaces the default instead of being capped by it, so a download can be faster than the others (but never above
  the total). The own speed is changed with `PATCH` and applies live to a running download (and when a queued, paused or
  failed one starts); clearing it hands the download back to the default. Changing the default applies live to the running
  downloads without an own speed. The engine holds the default and reads it under the same lock that registers a download,
  so a start cannot miss a change. Finished and cancelled downloads have nothing to limit (`409`).
- Settings changes are propagated live (watch channel).

### 6.3.2 Download hours and start times

For mobile connections with a night package, or a line that must stay free by day.

- **Download hours** (global, `Settings`): `schedule_enabled`, `schedule_from` and `schedule_to` (minutes after local midnight,
  `to` exclusive; `to` before `from` means over midnight, `22:00`-`06:00`; equal ends are refused while the hours are on) and
  `schedule_timezone`, an IANA zone (`Europe/Warsaw`). The server's own clock is usually UTC, so the zone is stated; the UI
  fills in the zone of the browser. Zones come from the database bundled in the binary (`jiff`), so a container without
  `tzdata` behaves the same and daylight saving moves the hours with the local clock. An unknown zone is refused (`400`).
- **Outside the hours** a download that is added or resumed stays `queued` without taking a slot (the manager keeps it
  `held`, outside the scheduler). What happens to the downloads that are *running* when the hours end is a setting,
  `schedule_on_end`: `pause` (the default, since a data package may end with the hours) stops them through the engine's
  pause, keeps their offsets and puts them back in the queue (held, not `paused`), and they continue from where they were
  when the hours open; `finish` lets them run to the end, and only downloads that have not started wait. The choice acts at
  once when it is saved, and a restart outside the hours puts what was running back in the queue in either mode. A check runs
  every 15 s and at once when the settings are saved, so switching the hours off or moving them acts immediately. The state
  survives a restart (a `queued` download is judged again at start-up).
- **"Download now"** (`start_now`, stored per download): the hours do not apply to that download; it is not held, and not
  stopped when they close. It is chosen when adding (`AddRequest.start_now`) or later for a waiting or running download
  (`PATCH {start_now: true}`, `409` for one that is not queued or downloading). It also drops a start time.
- **Start time** (`start_at`, unix seconds, `AddRequest.start_at`): the download waits until then (at most 31 days ahead, not
  in the past, not together with `start_now`). Meant for when there are no download hours; with hours on, both must allow it.
  Once begun, the start time no longer matters (a running download only obeys the hours).
- A user's own **pause** is a pause: it takes the download out of the hold and nothing starts it but "Resume" (which
  queues it again, and then the hours decide). A cancelled or removed download leaves the hold too.
- A waiting download is reported with `hold` (`window` or `time`) and `starts_at` (unix seconds): its start time or the next
  opening of the hours. They are computed, not stored.

### 6.3.1 Categories and directories

- Category = file extension first (servers often send `application/octet-stream`), then `Content-Type`
  (`image/*` Photo, `audio/*` Music, `video/*` Video, archive and installer MIME types), else `Other`.
  An explicit `category` in the add request wins; it can be changed later with `PATCH`.
- With `category_subdirs` on, a new download goes to `<download dir>/<category>/` (`photo`, `music`, `video`,
  `program`, `archive`, `other`). The subdirectory is stored per download (`downloads.subdir`) when it is added:
  changing the setting or the category afterwards never moves an existing file. The `.part` file lives next to the
  final file; conflict checks and name reservation are per subdirectory.

### 6.4 Data model (SQLite)

- `downloads`: id, url, filename, subdir, category, status, size, downloaded, source_kind, resumable, etag, overwrite, ignore_space, waiting_space, archived, speed_limit, start_at, start_now, active_ms, checksum_algo, checksum_expected, checksum_actual, verified, verify_failed, username, password, created_at, finished_at, error
- `segments`: download_id, idx, start, end, downloaded
- `known_hosts`: id, host, port, key_type, fingerprint, added_at (unique per host and port): the SSH servers the user accepted (6.12).
  `downloads.password` is read by `storage::downloads::credentials` only and is not a column of the `Download` model, so it cannot reach
  an API answer by accident.
- `history`: download_id, added_at, finished_at, outcome (`completed` | `failed` | `cancelled`), name, category, size, bytes,
  active_ms. One row per download that ended, written by a trigger on `downloads.status`, so whichever code path ends a
  download (also a removal while it runs) is recorded, and a retried download replaces its earlier row. It has no foreign
  key: removing an entry from the list leaves the statistics alone. `downloads.active_ms` is the time spent transferring,
  summed over pauses and restarts (the manager adds the time between progress ticks and closes it when a download pauses,
  ends or is cancelled); rows backfilled from before the table existed have `0` and stay out of the average speed.
- `settings`: key/value (connections, max_concurrent, speed_per_download, speed_global, category_subdirs, check_updates, schedule_enabled, schedule_from, schedule_to, schedule_timezone, schedule_on_end, check_prereleases)

Statuses: `queued`, `downloading`, `paused`, `verifying`, `completed`, `failed`, `cancelled`.

### 6.5 API (draft)

- `GET /api/downloads`, `POST /api/downloads`
- `POST /api/downloads/:id/keep` (6.2.2)
- `POST /api/downloads/:id/pause|resume|cancel`, `DELETE /api/downloads/:id[?delete_file=true]` (removes the entry; with
  `delete_file` also the finished file, only for a completed download and only the plain file name the engine finally
  chose, inside the download's own directory; a file that is already gone is fine, any other failure keeps the entry)
- `PATCH /api/downloads/:id` (`category`, `archived`, `start_now` (6.3.2) and/or `speed_limit` in bytes/s, `0` = back to the default speed: archiving hides a finished, failed or cancelled download from
  the main list, `409` for an unfinished one; the file stays where it is and the entry still counts for duplicate and name
  checks)
- `POST /api/probe {url, credentials?}` (looks at a URL without adding anything and answers `{kind, filename, size?, resumable, category,
  entries, truncated}`: the same probe an add makes, with the same URL policy and the same errors, `400` for a bad URL and
  `502` when the server cannot be reached; a `POST` so that an address with a token stays out of access logs. `kind` is
  `file`, or `listing` when the address is a directory index page, see 6.2.1, or an FTP or SFTP folder, see 6.12)
- `GET /api/known-hosts`, `POST /api/known-hosts {host, port, key_type, fingerprint}` (`201`; accepts the key, replacing an earlier one of the
  same host and port), `DELETE /api/known-hosts/:id` (`204`, `404`): the trusted SSH servers (6.12). `AddRequest.credentials` and
  `ProbeRequest.credentials` (`{username, password}`) carry a login; `Download.username` shows which one a download uses, its password is
  never returned.
- `409 {"error":"untrusted_host"|"host_key_changed","host_key":{kind,host,port,key_type,fingerprint,trusted_fingerprint}}` from `POST
  /api/probe` and `POST /api/downloads` when the SSH server of an `sftp://` address is not trusted, or shows another key than the accepted
  one (6.12). A refused login or a missing file at an FTP or SFTP server is `502` with the server's words, never `401` (that means that
  FetchBox's own session is missing).
- `GET /api/settings`, `PUT /api/settings`
- `GET /api/disk` (free space of the download disk)
- `GET /api/stats?range=today|7d|30d|all&offset=<minutes east of UTC>` (a summary of the downloads of a period, in the
  caller's days: `completed`, `failed`, `cancelled`, `in_progress` (not tied to the period), `bytes` fetched (plus what
  unfinished downloads have fetched so far), `completed_bytes`, `active_seconds`, `average_speed` (bytes over the time
  spent transferring, timed rows only), `peak_speed` (all time), `largest`, `categories` of the completed ones biggest
  first, and `days`: the last 14 days oldest first with the empty ones, whatever the period. Cancelling counts as neither
  success nor failure.)
- `GET /api/version` and `POST /api/version/check` (see 6.9): `{current, enabled, latest?, update_available, release_url?,
  checked_at?, check_failed, notes}`; the `POST` looks again now, at most every 30 s
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
  `docs/openapi.json`; `fetchbox --print-openapi` prints it. `utoipa` is an optional feature of `fetchbox-core`, so the core
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
- View switch: next to the sort controls the list header has two icon buttons, "Cards" and "Table" (`aria-pressed`, in a group labelled
  "View"). The choice is kept per browser with the sort and the filters (`fetchbox.list`, field `view`; a save from before the views has
  none and shows cards, with its sort and filters kept: `util/listPrefs`). Both views show the same filtered and sorted list and obey the
  same selection mode. The table (`DownloadTable`, `DownloadRow`) is a real `<table>`: name (category icon, file name, `PROTOCOL user@host`
  and folder, the error of a failed download in one truncated line, a warning icon for a file kept despite its checksum), status badge,
  progress bar with the percentage, size, speed, time left and the buttons as icons (the main one, "more" menu, cancel or remove, and "keep"
  for a failed verification), all with the same actions and confirmations as the card (`stores/downloadActions`, `util/downloads.primaryAction`).
  The column headers of name, status, progress, size and speed are buttons that sort by that key (a new key starts in its natural direction,
  the same key again reverses it, `aria-sort` marks the current one); "Date added" is only in the sort select, which stays in both views.
  Columns drop out as the width shrinks: time left, speed and size below 64rem, the progress column below 48rem (the bar and a short line
  of numbers move under the name), the status badge below 40rem. The table has no horizontal scroll and does not clip the card menu.
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
- Checksum (6.2.2): the add dialog has a checkbox "Verify the checksum" (single files only, not directory listings); ticked, it shows
  a type select (MD5, SHA-1, SHA-256, SHA-512) and a field for the checksum. Pasting picks the type (the label in the text, or
  else the length of the digest), choosing another type by hand sticks until the text changes, and what is wrong (not hexadecimal,
  the wrong length for the type) is said under the field; "Start" waits for a valid checksum. A card in `verifying` has the badge
  "Verifying", the bar shows how much of the file has been read, and its main button is "Skip verification" (`keep`). A completed
  and verified download shows a green tick and "Checksum verified (SHA-256)". A failed verification shows the expected digest and
  the one the file has (monospace, wrapped), a hint, and three choices: "Download again" (main button), "Keep the file" and the
  trash button, which opens the delete dialog (offering to take the file with it). `verifying` is part of the "Active" tab.
  A download that was kept although the checksum did not match is `completed` and shows a warning icon beside its "Completed"
  badge; the tooltip gives the expected and the real digest.
- Logins and SSH servers (6.12): the add dialog has the checkbox "This address needs a login" (always on and not removable for `sftp://`)
  with the user name and password (`autocomplete="new-password"`, so the browser does not offer FetchBox's own password), a line on where the
  password is kept and, for `http://` and `ftp://`, a warning that it travels unencrypted. An `sftp://` address is looked at only once
  there is a user name. When the probe or the add answers `409 untrusted_host`, the area under the address shows the fingerprint (and how
  to read it on the server) with "Trust this server", which posts to `/api/known-hosts` and looks again; `host_key_changed` shows both
  fingerprints and a button to Settings instead, and "Start" is disabled while either is shown. Settings has the card "Trusted SSH
  servers" (host, port, key type, fingerprint, date, "Forget" with a confirmation). A card of an FTP or SFTP download shows the protocol
  and `user@host`.
- A waiting download (`hold`) shows the badge "Waiting for the hours" or "Scheduled", "Starts 01:00" / "Starts tomorrow 02:30"
  instead of "Waiting for a free slot", and its main button is "Download now"; a pause button sits next to it.
- Card menu: the card's action row is the primary button (pause / resume / retry), a "more" button and, as before, cancel
  (unfinished) or "remove from list, the file stays" (finished). The "more" menu (replacing the old category select) has
  "Copy link" (the clipboard API where the page is a secure context, a selection fallback on plain-HTTP LAN pages),
  "Change category" (a second level with the current one checked), "Speed limit..." (only for a download that can still run;
  it shows the current limit) and "Delete...". It opens towards the side that has room
  (up or down, left- or right-aligned), closes on Escape / outside click / Tab, and works with the arrow keys, Home and End.
  "Delete..." opens a dialog (`stores/deleteDialog`): for a finished download it offers "Also delete the file from the disk"
  (off by default, with a warning when ticked, and a button that then reads "Delete with file"); for an unfinished one it
  says that the download is stopped and the partial file goes too, and has no such option.
  "Speed limit..." opens a dialog (`stores/speedDialog`, `SpeedLimitDialog`) with the same on/off + KB/MB field as Settings;
  switched off it says which default the download uses, switched on the value replaces that default. It saves with `PATCH`
  and takes effect at once. A card with an own speed shows "Limit N MB/s" in its stats line. Settings has the default speed
  of each download and the total limit.
- Download hours in Settings (6.3.2): a card "Download hours" with a switch; on, it shows "From" and "To" time fields, a summary
  ("Downloads run from 22:00 until 06:00 the next day"), and the time zone (a text field with the browser's list of zones,
  default the browser's zone while the hours are off), and a choice "When the hours end": "Pause running downloads" or
  "Let running downloads finish" (each with a line saying what it does). Equal times and an unknown zone are shown as errors
  before saving.
- Adding a download is a modal dialog (`AddDownloadDialog`), not a page: opened from the list header, the
  floating button and the empty list (all through `stores/addDialog`), mounted once in the app
  shell. The URL field is focused, Enter submits, Escape / Cancel / the backdrop close it and keep what was typed (a
  success clears it), the name-conflict and "does not fit" dialogs open on top of it, and a success closes everything and
  shows the list. Old `#/add` links still work: they open the dialog and the hash goes back to `#/`.
  The address field has a paste button (clipboard icon) at its right end: it reads the clipboard (a secure context and the
  browser's consent are needed; on a plain-HTTP LAN page it says so in a toast and Ctrl+V still works) and puts the first
  line in the field. A valid http(s) address is looked at with `POST /api/probe`, 0.5 s after the last keystroke or at once
  after a paste, and the result is shown under the field: the category icon, "Size: N" (or that the server does not report
  one) and, when the server does not support ranges, that the download cannot be resumed once paused; the file name
  field's placeholder becomes the name the server suggests. An older look is cancelled when the address changes. A
  failed look shows its reason but never blocks adding (the add reports a real problem itself).
  When to start (6.3.2) is one checkbox under the fields: with download hours on it is "Download now" (unchecked: it waits for
  the hours, which the hint names), without hours it is "Start at a set time" with a time field (the next time the local clock
  shows it, today or tomorrow, said in words below). It applies to every file of a directory listing too.
  When the address is a directory index (`kind: "listing"`) the file name field gives way to a list (`ListingPicker`): "Select
  all" (of what the filter shows), a filter box, the folders (a click opens one: the address becomes the folder's and
  "Back" returns) and the files with a checkbox, category icon and size, then "N selected" with the total size ("at least"
  when some sizes are unknown). Every file starts chosen. The category choice applies to all of them and the button reads
  "Download N files". Adding sends the chosen files one after the other, "Adding i of N..."; a file that already exists
  (`409`) or does not fit (`507`) is skipped and counted instead of asking, so one problem does not stop the rest, and one
  toast sums it up (a warning when something was skipped or failed). When nothing at all was added the dialog stays open
  with the reason.
- Motion: short and functional (about 150-250 ms), never decorative loops. Dialogs fade and rise in and fade and sink out
  (the native `<dialog>` is closed only after the exit has played; reopening during the exit cancels it); toasts fly in,
  fade out and the rest glide into place; the drawer scrim fades; the floating button pops in; a card added to an existing
  list rises in (the first card of an empty list and the cards of a freshly opened page do not animate, and the list is
  never re-ordered with animation because sorting by speed would make it jump every second); pages fade in on navigation;
  progress bars, the space gauge and the disk ring glide to new values; buttons give a small press feedback. Everything
  respects `prefers-reduced-motion`: CSS animations and transitions are cut to nothing in `app.css`, Svelte transitions take
  their duration from `util/motion.ts`, which is 0 then.
- Layout: side menu. From 64rem a permanent left panel (status and category filters with counts, where "All" is the way back to the list, then, pinned
  to the bottom, speed, the Statistics and Settings links, theme switch and the disk ring last; there is no separate
  "Downloads" link, and the filters are marked as current only while the list is shown, not on Statistics or Settings); below that the same panel is a drawer behind a top-bar button (`inert` page behind it, Esc and the
  backdrop close it, focus returns to the button) plus a floating add button on the list and tags for the applied filters.
  Choosing a filter on another page returns to the list. Forms use a narrow column; the card list is a responsive grid.
- First-run wizard (`routes/Setup.svelte`), shown while the server reports `setup_required` and until the user finishes
  it, kept as short as it can be: language, look (theme and accent colour; both applied live and stored like in
  Settings), password (setup code, password, repeat; this creates the account), and a final "All set" screen whose
  only button goes to the downloads. No download limits, speed unit or other options are asked: they have sensible
  defaults and live in Settings. Back goes to the previous step until the account exists; the final step already runs
  signed in, and reloading after the password step simply opens the app.
- Password: Settings has a "Password" card (the sign-out card comes after it: signing out is the last thing on the page). For an account created in the UI it has a "Change password..." button that
  opens a dialog (`ChangePasswordDialog`) with the current password, the new one and the new one again (hidden text,
  `autocomplete` hints for password managers). The checks are the server's (8 to 256 characters, both new entries equal)
  plus "not the same as the current one"; a wrong current password (`403`) and too many attempts (`429`) are shown in the
  dialog, and a success closes it with a toast. The server ends every other session, this one stays. Fields are emptied
  whenever the dialog opens. For a password from `FETCHBOX_PASSWORD` or `--password-file` the card only explains that it
  is set on the server. The login screen has a small "Forgot your password?" link that opens `ForgotPasswordDialog`:
  there is no e-mail reset (the server keeps only a hash), so it shows the commands to run on the machine that runs the
  server, each with a copy button: for Docker Compose `docker compose stop fetchbox`, `docker compose run --rm fetchbox
  reset-password`, `docker compose up -d fetchbox`, `docker compose logs fetchbox`; without Docker `fetchbox --data-dir
  ./data reset-password`; then enter the setup code from the log (see 6.7). With an external password the dialog says to
  change it where it is set and restart.
- Start-up speed (the app is used over Wi-Fi, VPN or a phone connection to small hardware, so round trips and bytes
  matter): the server sends hashed build output (`assets/*`) with `Cache-Control: public, max-age=31536000, immutable`
  and everything else (`index.html`, manifest, icons) with `no-cache` plus an `ETag`, so a repeat visit costs one `304`
  and loads nothing else. Text files are served precompressed: `npm run build` writes `<file>.br` and `<file>.gz` next
  to them (Node's own zlib, in `vite.config.ts`), the server embeds them and picks the variant from `Accept-Encoding`
  (Brotli before gzip, each with its own `ETag`, `Vary: accept-encoding`); the server compresses nothing at request
  time. `index.html` starts `GET /api/auth/status`, `/api/downloads` and `/api/settings` itself before the bundle has
  loaded (`window.__fetchboxEarly`, `api/early.ts`); the app takes those answers once instead of asking again, falls back to
  a normal request when one failed or is not `ok` (before sign-in two of them are `401`, which must never sign anyone
  out), and ignores them after 10 s. The session check also runs while the language file loads, and the list is
  fetched while the event stream opens (the reload on open stays: it closes any gap between the two). The first disk
  reading is the app's own (it refreshes whenever the list changes), not asked twice.
- Reconnecting: the event stream starts with a comment and `retry: 1000` (a browser then reconnects after one second, not
  about three) and carries `X-Accel-Buffering: no`, so a reverse proxy such as nginx does not hold it back until the
  first keep-alive. The client retries with 0.5, 1, 2, 4, then 5 s steps, reconnects at once when the tab becomes
  visible or the network comes back (and refreshes the list when the stream looked alive, since a background tab may
  have frozen it), and the "server unreachable" screen tries again by itself with the same steps.
- Lost connection: there is no status line for the connection (when everything works there is nothing to show). Once the
  event stream has been open, a stream that is not live covers the whole app with a modal dialog that cannot be dismissed
  (no Escape, no backdrop click; the page behind is inert): a spinner, "Connection lost", "Trying to reconnect to the
  server..." and "Downloads keep running on the server." It appears only after 1.5 s without a connection, so a quick
  restart or a network hiccup does not flash it, and goes away by itself when the stream is back (or when the session
  ended and the login screen takes over). The same dialog covers the two other ways of being cut off: the server is not
  there when the app starts (the page behind says "Loading..."; the session check retries by itself), and the server
  dies while the login screen or the first-run wizard is up. Nobody can check a password then, so none is asked for:
  the login screen stays mounted under the dialog (what was typed is kept), keeps asking `GET /api/auth/status` every
  3 s (not while the tab is hidden; coming back to the tab asks at once) and a sign-in that fails because the server
  is gone checks immediately. When the server answers again the dialog goes away and the screen is usable as before.
  "Not there" includes a reverse proxy that answers for a dead backend (502/503/504, or 500 from the Vite dev proxy):
  `GET /api/auth/status` is public and the server always answers it, so any other answer is never "please sign in".
  A signed-in app stays on screen under the dialog (nothing is torn down); it goes to the login screen only when the
  server is back and says the session is gone (a restart forgets sessions).
  The spinner is the one animation kept under `prefers-reduced-motion` (slower, not frozen): it says "still trying".
- SSE store keeps list in sync; REST for commands. Command responses are not patched into the list (they can be older
  than the event that followed); the stream is the single source of truth, and the list is re-fetched on every
  (re)connect and on `resync`.
- Sorting (date added, name, size, progress, speed, status; unknown sizes always last), status tabs, category filter,
  search; remembered in `localStorage`. System / light / dark / black theme via CSS tokens (`[data-theme]` plus
  `prefers-color-scheme`; black is dark with a pure black background for OLED), applied before first paint.
- Speed unit shown in the app: a display choice in Settings > Appearance (not in the setup wizard), kept per browser in
  `localStorage` (`fetchbox.speedUnit`), not a server setting. `Automatic` (B, KB, MB... as before), `Automatic in bits` (bit,
  Kbit, Mbit, Gbit...) or one fixed unit (KB/s, MB/s, Kbit/s, Mbit/s). It applies to every speed the UI writes (list header,
  side panel, cards, limits on cards and in menus); the limit fields keep the unit of their own field. Fixed and bit units
  show three significant figures (123, 12.3, 1.23).
- Statistics page (`routes/Stats.svelte`, "Statistics" in the side menu, `#/stats`): a period switch (today, 7 days, 30 days,
  all time; remembered in `localStorage`, `fetchbox.statsRange`), a headline figure (the data downloaded, >= 48 px) with the
  number of downloads, tiles (downloads, completed with the success rate = completed / (completed + failed), failed,
  cancelled, average speed, peak speed, time spent downloading, largest file), the completed downloads per category as
  horizontal bars (one hue; the label and icon carry the identity; size and count at the tip) and the last 14 days as
  columns of data per day (one series, so no legend; a hairline grid with a clean top in the unit it is written in, columns
  <= 24 px with a rounded top, a tooltip per column, a hit target the width of the day, a table for screen readers). Speeds
  follow the speed unit of the app. The numbers reload every 10 s while the page is open (not in a hidden tab) and as soon
  as a download ends. Nothing to count yet shows an explanation instead of zeroes. Because the numbers come from the
  history, they stay when downloads are removed from the list.
- Dialogs: the buttons of a dialog (`Modal` footer) stay at the bottom of the dialog while its content scrolls, so a long
  dialog (update notes, the file list of the add dialog on a phone) never hides them below the fold; content columns are
  `minmax(0, 1fr)` so a long command or file name scrolls or wraps inside the dialog instead of widening it.
- Select boxes (language, speed unit, category, sort, field units) are drawn like the text inputs: same height, border, font
  and hover/focus, no native arrow but one chevron in every theme. Number inputs have no native spin buttons.
- Settings form works in KB/s, MB/s, Kbit/s and Mbit/s and "limit on/off"; the API stays in bytes/s with `0` = unlimited.
  Bytes are binary (1 KB = 1024 B), bits decimal as network speeds are quoted (1 Mbit/s = 125 000 B/s). The server does
  not store the unit: a stored speed is shown in the first unit (MB, KB, Mbit, Kbit) that gives a whole number, so
  100 Mbit/s comes back as 100 Mbit/s. The same unit picker is in the speed dialog of a download. The form follows server
  changes made by other clients unless the user has unsaved edits. While there are unsaved edits a bar is fixed to the
  bottom of the viewport (next to the side panel on wide screens) with "Discard" and "Save changes" and a failed save's
  message, wherever the page is scrolled; the page gets room below so nothing hides behind it.
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
  `FETCHBOX_PASSWORD` / `--password-file` (external: wins, cannot be changed in the UI), the account created in the
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
- Forgotten password: `fetchbox [--data-dir D] reset-password` removes the stored hash (`settings.password_hash`) and
  nothing else, so the next start of the server is a first run: locked, with a fresh setup code in its log, and a new
  account is created in the browser. Downloads and settings are kept. The server must be stopped while it runs (it holds
  the hash in memory, and two processes should not share a SQLite file over some container file systems); it opens the
  database only if it exists (a wrong `--data-dir` is an error, not an empty database). It never starts the server or
  creates the download directory, so it works with the flags of the Docker entrypoint in front of it. A password from
  `FETCHBOX_PASSWORD` / `--password-file` is not affected (a warning says so) and an empty `FETCHBOX_PASSWORD` counts as
  not set. A reset needs access to the host or the container, the same access that could read the database anyway; there
  is deliberately no reset from the login screen.
- Logins of sources (6.12): stored in `downloads.username` / `downloads.password` (plain text: the download needs the password again after a
  pause or a restart, so a hash would be no use), never part of an API answer, an event or a log line, and shown in `Debug` output as
  `***`. A login in an address is taken out of it before anything is stored or shown. The database is as private as the data directory.
- Sanitize filenames, never write outside download dir.
- Optional blocklist of private IP ranges for URLs (SSRF), `--block-private-urls`, off by default because fetching
  from a NAS in the LAN is normal use. It blocks loopback, RFC 1918, link-local (cloud metadata), CGNAT, multicast,
  unspecified and IPv6 unique-local/link-local addresses (IPv4-mapped IPv6 is judged by its IPv4 part), and is
  enforced in three places so it cannot be bypassed: IP literals in the URL, a filtering DNS resolver for host
  names (also stops DNS rebinding), and every redirect hop. Violations are `400` (literal) or `502` (name).
- Crash safety: progress is persisted once per tick; after a kill the interrupted downloads are re-queued and resume
  from the saved segment offsets (verified by a test that kills a runtime mid-download).

### 6.8 Shutdown

- `SIGINT` (Ctrl-C) and, on Unix, `SIGTERM` (`kill`, `docker stop`, launchd, systemd) start a graceful shutdown: the
  listener closes, requests in flight finish, and the event streams (`/api/events`), which never end by themselves, are
  ended by the server (`Service::closing`). Without that a single open browser tab kept the process alive for ever.
  Connections that still hang on after 5 s (a client that never finished its request) are dropped, and a second signal
  exits at once (status 130). When the async runtime is torn down it waits at most 3 s for blocking work (a name lookup
  stuck in the OS, a slow disk) instead of for ever. Running downloads need no step of their own: offsets are persisted
  every tick and whatever was running is re-queued at the next start (see "Crash safety"); at most one tick of progress
  is fetched again.

### 6.9 Version and updates

- The version is the workspace version in `Cargo.toml` (`0.6.1` while there is no final release); the binary prints it with
  `--version`, the API with `GET /api/version` and Settings shows it in the "About" card (also in `docs/openapi.json`, whose
  `info.version` it is). `web/package.json` carries the same number.
- Update check: the server asks the GitHub releases API for the list of releases (`/repos/<owner>/<repo>/releases?per_page=20`;
  drafts are skipped, pre-releases are kept in the cached answer and shown only with `Settings.check_prereleases`) with a 5 s timeout, takes the highest version as `latest` and compares its tag
  (`v0.7.0` -> `0.7.0`, numbers compared one by one, a pre-release below its release, the pre-releases of one version in the order
  of semantic versioning: `rc.1` < `rc.2` < `rc.10`) with its own version. `latest_prerelease` says that `latest` is one, and
  each note carries `prerelease`. `notes` holds the
  description (Markdown, at most 8000 characters) of each release newer than the running version, newest first, at most ten:
  what changed since the user's version. An answer is kept for 6 hours, a failure for 15 minutes (an offline
  server does not retry on every page view), and "Check now" cannot ask more often than every 30 s (anonymous GitHub limit: 60
  requests an hour). An empty list or a `404` means no release has been published yet and is not an error. The release link is shown only
  when it is an `https://` address. Nothing is sent but the request itself (the server's IP address and
  `User-Agent: FetchBox/<version>`).
- It can be switched off (`Settings.check_updates`, default on; a client that does not send the field leaves it on). Then
  `GET /api/version` answers `enabled: false` and the server contacts nobody. `FETCHBOX_UPDATE_URL` replaces the endpoint (a
  mirror, a fork, a test server).
- Settings "About" card (after the password card, before the sign-out card): `FetchBox <version>`, one line of status ("Version X
  is available" with a "View the release" button, "You have the latest version", "No release has been published yet", "Could
  not check", "switched off"), the time of the last answer, "Check now" and the switch "Check for updates automatically"
  (part of the settings form: saved with it, and the answer is fetched again once saved).
- Settings "Check now" always answers with a toast: the latest version (success), a newer one (info, and the update dialog
  opens), no release published yet, switched off, or could not check (warning).
- Update dialog (`UpdateDialog`, `stores/updateNotice`): "Version X is available", "You are running version Y", "What is new" (the
  notes of every newer release in a scrolling box, each with a link to GitHub) and "How to update": Docker Compose (as in the
  repository: `git pull`, `docker compose up -d --build fetchbox`; data in `./data` and `./downloads` stays, unfinished
  downloads continue after the restart) and without Docker (`git pull`, `./scripts/build.sh`, start the new binary), each
  command with a copy button. Notes are drawn as text nodes from a small Markdown reading (headings, lists, paragraphs, `**bold**`,
  `code`; link targets dropped), never as HTML (`util/notes`). After signing in the app looks once and, if an update is known,
  opens the dialog with "Remind me later" (hides it for that version for 24 hours; Escape and the backdrop mean the same) and
  "Skip this version" (never announces that version again; a later one is announced); both are kept per browser in
  `localStorage` (`fetchbox.updateSnooze`, `fetchbox.updateSkip`). From Settings ("How to update" in the About card) it opens
  with a plain "Close". It does not open while update checks are off. The About card has a second switch, "Also announce
  pre-releases" (disabled while checks are off; saved with the form; Settings and the update dialog use "Pre-release X" and a
  "pre-release" badge, and the dialog says that the commands below install the stable version).
- The app does not update itself. In the container the image is read-only (distroless, no shell, no package manager) and a
  restart or `docker compose up` brings back the image's version, so a self-replaced binary would not survive; updating the
  container from inside would also need access to the Docker socket, which is root on the host. A user who wants unattended
  updates can run an updater such as Watchtower with a published image; that is outside the app.
- A release is made by pushing the tag `v<version>` (for example `v0.6.1`): `.github/workflows/release.yml` checks that the tag
  equals the workspace version, builds the web UI and the binary for x86_64 and aarch64 Linux (in a Debian 12 container on purpose: the glibc
  it is linked against is the oldest it runs on, so it runs on Debian 12+ and Ubuntu 22.04+; the Docker image and the Proxmox
  container are Debian 13), wraps each binary in a DEB and an RPM (6.11), installs every package in a clean container as a test
  and publishes a GitHub Release with `fetchbox-linux-<arch>.tar.gz` (just the binary, the web UI is inside it),
  `fetchbox_<version>_<amd64|arm64>.deb` and `fetchbox-<version>-1.<x86_64|aarch64>.rpm`, and a `.sha256` file for each, with
  generated notes; a tag with a dash is a pre-release (the update check announces it only to those who switched on pre-releases;
  the installers' own update commands take only stable releases). That release is what the update check finds and what the installers
  (`deploy/install.sh`, `deploy/proxmox`) install and update from; nothing is compiled on the user's machine. Raising the version
  means changing `Cargo.toml` (and `web/package.json`) first: `scripts/bump-version.sh <version>` changes every file that carries it. When
  such a change reaches `main`, `.github/workflows/tag.yml` tags the version if it has no tag yet and starts `release.yml` on the tag
  (a tag pushed with the workflow token starts no workflow by itself, so the dispatch is explicit; `release.yml` also runs on a tag
  pushed by hand and can be run by hand on a tag's ref).

### 6.10 Console log

The server logs what a user would want to look up later, one line per event. Start-up information, warnings and errors are always
logged; the per-event lines below are `info` lines with the tracing target `activity` and are shown only with `--log-activity`
(or `FETCHBOX_LOG_ACTIVITY=true`), so a default install stays quiet. `RUST_LOG` sets the level of everything else
(`RUST_LOG=debug` adds the rest). Values are `key=value` pairs after the message; sizes are binary (`7.8 MiB`), speeds `MiB/s`.

- Start-up (always): the version, the data and download directories, the listening address, the settings in force (in words) and the
  number of downloads per status.
- Downloads: added (id, name, category, size, host of the address), started (segments, resumable), paused, resumed (offset),
  completed (size and the average speed over the time spent transferring), failed (reason), cancelled, category and own
  speed changed, "download now", waiting for the download hours (and when they open), and removed from the list (status it had,
  size, bytes fetched, average speed and whether the file was deleted too). Refused adds are `warn`.
- Settings: one line listing only what changed (`connections 4 -> 8; download hours off -> 22:00-06:00 Europe/Warsaw`).
- Download hours: they opened or closed, how many downloads were released or stopped, and what the end does.
- Sign-in and sign-out; a newer version found by the update check; an SSH host key accepted or forgotten (host, port, fingerprint).
- Never logged: passwords, tokens, setup codes, session ids, and the path and query of a URL (only its host, because an address
  may carry a token). Progress ticks and the 15 s hours check log nothing unless something changes.

### 6.11 Packages and installers

- **Packages** (`packaging/`): one nfpm description (`nfpm.yaml`) makes the DEB and the RPM of an architecture from the release
  binary (`packaging/build.sh <version> <amd64|arm64> <binary> <outdir>`; nothing of the binary is executed, so both CPUs are
  packaged on one runner). Contents: `/usr/bin/fetchbox`, `/usr/lib/systemd/system/fetchbox.service`, `/etc/fetchbox/fetchbox.env`
  (a config file: an upgrade keeps the administrator's edits; mode 0600 because it may hold the password) and the licence. They
  depend on glibc 2.36 (the builder's), so Debian 12+, Ubuntu 22.04+ and Fedora install them and RHEL 9 is refused by the package
  manager. A pre-release `0.7.0-rc.1` is `0.7.0~rc.1` inside the package (sorts below `0.7.0`) while the file names keep the tag's
  spelling.
- **Layout and defaults**: the service runs as the system user `fetchbox`; database and working directory `/var/lib/fetchbox`,
  downloads `/var/lib/fetchbox/downloads`, port 8080. The unit sets these through `FETCHBOX_DATA_DIR`, `FETCHBOX_DOWNLOAD_DIR` and
  `FETCHBOX_PORT` (the options `--data-dir`, `--download-dir` and `--port` read the same variables; `--block-private-urls` reads
  `FETCHBOX_BLOCK_PRIVATE_URLS`, `--log-activity` `FETCHBOX_LOG_ACTIVITY`), and `/etc/fetchbox/fetchbox.env`, loaded after them,
  overrides them, so changing the port or the download folder needs no edit of the unit. A variable must not be empty. The unit
  hardens the service (`NoNewPrivileges`, `PrivateTmp`, `PrivateDevices`, `ProtectSystem=full`, kernel protections) but leaves the
  rest of the file system writable, because the download folder can be anywhere.
- **Maintainer scripts** (POSIX `sh`, the same file for both formats; they tell a first install from an upgrade by the arguments of
  `dpkg` and `rpm`): after installing, create the user `fetchbox` if it does not exist (the Proxmox installer makes it first with
  a fixed uid) and `/var/lib/fetchbox` (0750); with systemd running, a first install enables and starts the service and prints the
  one-time setup code, an upgrade only restarts a service that is running (one that was stopped stays stopped). Without systemd
  (an image build, a chroot) only the files are put in place. Before removing, the service is stopped and disabled (not on upgrade).
  Removal keeps the database, the downloads and the user; `apt purge` also removes the settings file.
- **Tests**: `packaging/smoke-test.sh <package> <version> [--run]` installs a package in a container without systemd, checks the
  files, the user and the permissions, with `--run` starts the program as `fetchbox` and requires `GET /api/health` to answer, then
  removes (and purges) it and checks that the data stays. CI runs it on every pull request with a stub program (`packaging` job: also
  `shellcheck` of all scripts); the release workflow runs it with the real binary on Debian 12 and Fedora, on x86_64 and on ARM
  runners, before anything is published. A failing test stops the release.
- **Installers**: `deploy/install.sh` (any Linux with systemd: downloads the tarball, checks its checksum, creates the user and the
  unit; `update`, `uninstall [--purge]`) refuses to run when a package of FetchBox is installed, since both own the same program and
  service. The Proxmox container installer installs the `.deb` (it creates the user with uid 1000 first, adds a drop-in that moves
  the downloads to `/downloads` and checks the `.sha256`), and its `update` installs the newer `.deb` over the old one; a container
  from the first versions (program in `/opt/fetchbox`) is moved to the package by `update`, carrying the database and settings over.
  The repository does not run a package repository: `apt` and `dnf` do not see new releases by themselves.

### 6.12 FTP, FTPS and SFTP sources

- **Addresses.** `ftp://[user[:password]@]host[:port]/path` (plain FTP, port 21), `ftps://...` (FTP over TLS: explicit, `AUTH TLS` on the
  given port, or implicit, TLS from the first byte, when the port is 990) and `sftp://...` (port 22). The scheme is all that decides
  (`SourceKind::for_url`, `fetchbox_http_engine::source::parse`); `ftp` and `ftps` are one `source_kind`. FTP paths follow RFC 1738: relative
  to where the login starts, `//` for the root. SFTP paths are absolute, and `/~/` starts in the home folder (as curl reads them). A
  path with a control character (a line break would be a second FTP command) is refused when the address is parsed, as is a login with one.
- **Logins.** `AddRequest.credentials` / `ProbeRequest.credentials`, or `user:password@` in the address (escapes undone). Whatever is given
  in the address is taken out of it first (`source::split_login`), so the stored and shown `url` never has a password; an explicit
  `credentials` wins. FTP without a login is `anonymous`; SFTP has none (an error before connecting). HTTP gets the login as basic
  authentication on the probe, the listing and every segment (`reqwest` drops it on a redirect to another host). `Job.credentials` is read
  from the database when a download starts, so resuming and restarting need nothing from the browser.
- **One engine.** `worker::Transport` (`Http`, `Ftp`, `Sftp`) opens the file at an offset and returns a `ChunkSource`; writing, counting,
  rate limiting, checkpoints (6.2), events and the segment bookkeeping are the same code for all. FTP and SFTP use a single segment
  (`segments::plan(size, 1)`): servers often allow few connections per user, and a pipelined SFTP read or one TCP stream fills a line. A file
  is resumable when its size is known and the server can continue (FTP: `REST`; SFTP: always). `RemoteInfo.etag` is `mtime:<the server's
  modification time>` (`MDTM`, SFTP `mtime`), so a resume starts over when the file changed. A refused connection and a dropped one are
  retried like an HTTP error (`EngineError::is_retryable`: FTP `4xx`, network errors); a refused login, a missing file and an unknown host
  key are not. The download hours, limits and the disk checks of 6.2 and 6.3 apply unchanged.
- **FTP client** (`ftp.rs`, no dependency): control connection with multi-line replies (bounded), `USER`/`PASS`, `FEAT`, `OPTS UTF8 ON`,
  `TYPE I`, `SIZE`, `MDTM`, `REST`, `RETR`, `MLSD` (else `LIST` in the Unix or the DOS style), passive mode only: `EPSV`, else `PASV`. The
  address a server names in a `PASV` reply is ignored: the data connection goes to the address of the control connection, so a server
  cannot aim this program at another machine (and a server behind NAT that names its inside address still works). Timeouts: 15 s to
  connect, 30 s for a reply, 30 s of silence on a transfer. The data connection is closed (with the TLS closing handshake) before the final
  reply is read, which some servers need before they send it.
- **FTPS.** `AUTH TLS`, `PBSZ 0`, `PROT P`; the data connections are protected too and use the same TLS client config, so they can resume
  the control connection's session (vsftpd requires that by default). The certificate is verified against the roots of `webpki-roots`
  (the same as for HTTPS) and the name of the host; a server that does not answer `AUTH TLS` is an error, never a plain connection.
  Self-signed certificates are not supported yet.
- **SFTP** (`sftp.rs`, `russh` and `russh-sftp`): the host key is checked in the handler before any authentication, so no password is
  sent to a server nobody vouched for. `HostKeys::trusted(host, port)` (the engine's trait, implemented by the server over an in-memory copy
  of `known_hosts`) returns the accepted `SHA256:` fingerprint; none is `HostKeyProblem::Untrusted`, a different one `Changed`, both
  carrying the fingerprint and key type that were offered, and both reach the user as `409` (6.5). The user accepts a key with
  `POST /api/known-hosts` after comparing the fingerprint with the server's own; there is no trust on first use. Only password logins
  exist. Reads are pipelined by the SFTP library (16 requests in flight), seeking is `SeekFrom::Start(offset)`.
- **Folders.** An address that is a folder (an FTP address ending in `/` or answering `CWD`, an SFTP directory) is listed through
  `POST /api/probe` as `kind: "listing"`, like a directory index page (6.2.1): folders first, then files, each by name, entries with an
  unusable name (`/`, `\`, control characters, `.` and `..`) dropped, at most 2000 (`truncated`), each with an absolute address (the login
  is not in it: the add dialog sends the same login with every chosen file). A folder is not a file: adding it is `400`.
- **Policy.** `--block-private-urls` applies as for HTTP: an address written as a number is checked up front, a name is resolved by the
  engine itself (`net::connect`), which drops the blocked addresses and connects to one it resolved, so a name cannot be rebound to a
  private address afterwards.
- **Not supported yet.** SFTP private keys and agents, FTPS with a self-signed certificate, active-mode FTP, connecting through a proxy,
  more than one connection per download, and changing a login after the download was added.
- **Tests.** The engine's tests run against in-process servers (`testftp`: users, `REST`, listings in three styles, TLS explicit and
  implicit, cut transfers, refused connections, a lying `PASV`; `testsftp`: `russh` with `russh-sftp`, password logins, a cut transfer),
  which the `test-servers` feature also gives to the server crate's tests. Interoperability was checked by hand against vsftpd (plain
  and TLS with `require_ssl_reuse`) and OpenSSH.

## 7. Dependencies (Rust)

`tokio`, `axum`, `reqwest` (stream, rustls), `rusqlite` or `sqlx` (SQLite), `serde`, `serde_json`, `tracing`, `tracing-subscriber`, `clap`, `thiserror`, `anyhow`, `async-trait`, `governor` (or custom bucket), `rust-embed`, `mime_guess`, `utoipa`, `tower-http`, `argon2` (password hash), `jiff` (time zones for the download hours, with its tz database bundled so no `tzdata` is needed), `rustix` (free disk space via `statvfs`, no `unsafe`; Unix), `russh` and `russh-sftp` (SFTP; pure Rust with the `ring` backend, about a hundred
mostly RustCrypto crates, about 4 MB of the binary), `tokio-rustls`, `rustls` and `webpki-roots` (FTPS; already built for `reqwest`) and `bytes`.
Tests: `rcgen` and `rand` (a throwaway certificate and a host key) behind the engine's `test-servers` feature. Later: `librqbit`.

## 8. Milestones

1. Core: model, SQLite, scheduler, `HttpDownloader` without segments.
2. API + SSE + minimal frontend (list, add).
3. Pause/resume and segmented downloads.
4. Rate limits and settings UI.
5. PWA and mobile polish, auth.
6. BitTorrent via `librqbit`.
