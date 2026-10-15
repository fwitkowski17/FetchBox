# FetchBox — specification

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
  - default speed of each download (a single download can get its own speed from its menu, which replaces the default, see 6.6)
  - total speed limit of all downloads together
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
FetchBox/
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

### 6.3.1 Categories and directories

- Category = file extension first (servers often send `application/octet-stream`), then `Content-Type`
  (`image/*` Photo, `audio/*` Music, `video/*` Video, archive and installer MIME types), else `Other`.
  An explicit `category` in the add request wins; it can be changed later with `PATCH`.
- With `category_subdirs` on, a new download goes to `<download dir>/<category>/` (`photo`, `music`, `video`,
  `program`, `archive`, `other`). The subdirectory is stored per download (`downloads.subdir`) when it is added:
  changing the setting or the category afterwards never moves an existing file. The `.part` file lives next to the
  final file; conflict checks and name reservation are per subdirectory.

### 6.4 Data model (SQLite)

- `downloads`: id, url, filename, subdir, category, status, size, downloaded, source_kind, resumable, etag, overwrite, ignore_space, waiting_space, archived, speed_limit, created_at, finished_at, error
- `segments`: download_id, idx, start, end, downloaded
- `settings`: key/value (connections, max_concurrent, speed_per_download, speed_global, category_subdirs)

Statuses: `queued`, `downloading`, `paused`, `completed`, `failed`, `cancelled`.

### 6.5 API (draft)

- `GET /api/downloads`, `POST /api/downloads`
- `POST /api/downloads/:id/pause|resume|cancel`, `DELETE /api/downloads/:id[?delete_file=true]` (removes the entry; with
  `delete_file` also the finished file, only for a completed download and only the plain file name the engine finally
  chose, inside the download's own directory; a file that is already gone is fine, any other failure keeps the entry)
- `PATCH /api/downloads/:id` (`category`, `archived` and/or `speed_limit` in bytes/s, `0` = back to the default speed: archiving hides a finished, failed or cancelled download from
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
- Layout: side menu. From 64rem a permanent left panel (navigation, status and category filters with counts, speed, theme
  switch, disk ring); below that the same panel is a drawer behind a top-bar button (`inert` page behind it, Esc and the
  backdrop close it, focus returns to the button) plus a floating add button on the list and tags for the applied filters.
  Choosing a filter on another page returns to the list. Forms use a narrow column; the card list is a responsive grid.
- First-run wizard (`routes/Setup.svelte`), shown while the server reports `setup_required` and until the user finishes
  it: look (language, theme, accent; applied live, stored like in Settings), account (setup code, password, repeat),
  download defaults (the same fields as Settings, optional), done. The steps after the account one run signed in;
  reloading in between simply opens the app.
- Password: Settings has a "Password" card. For an account created in the UI it has a "Change password..." button that
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
- Speed unit shown in the app: a display choice in Settings > Appearance (and the setup wizard), kept per browser in
  `localStorage` (`fetchbox.speedUnit`), not a server setting. `Automatic` (B, KB, MB... as before), `Automatic in bits` (bit,
  Kbit, Mbit, Gbit...) or one fixed unit (KB/s, MB/s, Kbit/s, Mbit/s). It applies to every speed the UI writes (list header,
  side panel, cards, limits on cards and in menus); the limit fields keep the unit of their own field. Fixed and bit units
  show three significant figures (123, 12.3, 1.23).
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

## 7. Dependencies (Rust)

`tokio`, `axum`, `reqwest` (stream, rustls), `rusqlite` or `sqlx` (SQLite), `serde`, `serde_json`, `tracing`, `tracing-subscriber`, `clap`, `thiserror`, `anyhow`, `async-trait`, `governor` (or custom bucket), `rust-embed`, `mime_guess`, `utoipa`, `tower-http`, `argon2` (password hash), `rustix` (free disk space via `statvfs`, no `unsafe`; Unix). Later: `librqbit`.

## 8. Milestones

1. Core: model, SQLite, scheduler, `HttpDownloader` without segments.
2. API + SSE + minimal frontend (list, add).
3. Pause/resume and segmented downloads.
4. Rate limits and settings UI.
5. PWA and mobile polish, auth.
6. BitTorrent via `librqbit`.
