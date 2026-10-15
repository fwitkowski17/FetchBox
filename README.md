# FetchBox

Self-hosted download manager for a homelab. One Rust binary serves the REST API, SSE and the embedded Svelte frontend. See [docs/SPEC.md](docs/SPEC.md).

Status: usable; BitTorrent support is planned.

## Develop

```bash
cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads
cd web && npm install && npm run dev   # Vite proxies /api to 127.0.0.1:8080
```

Checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && npm run check
```

## Frontend

Svelte 5 + Vite + TypeScript in `web/`, mobile-first, styled with Tailwind CSS 4 (no UI component framework; about 40 kB gzipped JS and 7 kB gzipped CSS, 316 kB for the whole embedded app including the font). It is embedded into the binary at build time; `cd web && npm run dev` serves it with hot reload and proxies `/api` to `localhost:8080`.

- Side-menu layout: on wide screens a permanent panel with navigation, status and category filters (with counts), connection state and the theme switch; on phones the same panel slides in from the left, with a floating add button and a row of the filters currently applied.
- Download list as cards with live progress (server-sent events): status, size, speed, ETA, category, pause/resume/retry, cancel, remove.
- Sorting (date, name, size, progress, speed, status; either direction), status and category filters and search; the choices are remembered.
- Add by URL with optional name and category; name conflicts open a dialog (keep both / replace / cancel).
- Settings: connections, simultaneous downloads, speed limits (KB/s or MB/s), category folders. Applied live; other open tabs follow.
- Light, dark, black (pure black background for OLED screens) or follow-the-system theme, plus a choice of eight accent colours (blue, violet, pink, red, orange, green, teal, graphite); both are applied before first paint. Login screen, automatic return to it when the session ends.
- Status filters (all, active, paused, completed, failed) have icons coloured by state, in the side panel and in the filter tags on phones.
- English and Polish, chosen from the browser language or in Settings; numbers, dates and name sorting follow the language.

### Adding a language

Translations live in `web/src/lib/i18n/` (no library; a few hundred lines). To add one, say German:

1. Copy `pl.ts` to `de.ts` and translate the values. Keep the `{placeholders}` as they are; a message that depends on a count is an object with the plural forms of that language (`one`, `few`, `many`, `other`, whatever the language needs; see `downloads.cleared`).
2. Add one line to `locales.ts`: `{ code: 'de', name: 'Deutsch', load: async () => (await import('./de')).default }`.

That is all: the language appears in Settings, is loaded on demand as its own small chunk, and is picked automatically when it matches the browser language. `npm run check` fails on a missing or misspelled key, and `npm test` fails on a changed placeholder or a missing plural form. `en.ts` is the source of truth for the keys. Messages coming from the backend (for example the reason a download could not be added) are not translated yet.

`npm run check` (types), `npm test` (vitest, logic only) and `npm run build` run in CI.

### Styling and font

Accent colours are one hue on top of the theme: lightness and saturation come from the theme (`--accent-l`, `--accent-c` in `app.css`, which is what keeps text on the accent readable), the hue is the user's choice (`web/src/lib/util/accent.ts`, a table of ids, hue and a saturation factor). A test computes the contrast of every accent in every theme from the real CSS values and fails below WCAG AA, so a new accent or a changed token cannot silently make buttons or links unreadable. Adding an accent is one line in that table plus its name in each dictionary.

Tailwind runs at build time only (the final image still has no Node). Colours are CSS variables defined per theme in `web/src/app.css` and exposed to Tailwind through `@theme inline`, so classes such as `bg-surface`, `text-muted` or `border-border` follow light, dark and black without any `dark:` variants. Shared controls (`.btn`, `.input`, `.select`, `.field`) are defined once in the same file with `@apply`.

The font is Inter (variable), bundled with the app, so nothing is fetched from a CDN and it works on an isolated LAN. Only the `latin` (48 kB) and `latin-ext` (85 kB, Polish letters among others) subsets are included, and the browser downloads a subset only when a character from it is on the page. Tailwind 4 needs a reasonably recent browser (Safari 16.4+, Chrome 111+, Firefox 128+).

## Options worth knowing

- `--block-private-urls`: refuse URLs that point at loopback, private, link-local or similar addresses (SSRF protection). Off by default so you can download from a NAS or another host in your LAN; turn it on if the manager is reachable from outside.
- Settings (in the UI/API): connections per download, max simultaneous downloads, per-download and global speed limits, and sorting downloads into per-category subdirectories. All apply live.

## API documentation and types

The HTTP API is described by an OpenAPI 3.1 document generated from the Rust code (`utoipa`). It is served at `/api/openapi.json` (public) and committed as [docs/openapi.json](docs/openapi.json). The frontend types in `web/src/lib/api/schema.d.ts` are generated from it and must not be edited by hand.

After changing anything in the HTTP API run:

```bash
./scripts/gen-api.sh   # rewrites docs/openapi.json and web/src/lib/api/schema.d.ts
```

A test fails when `docs/openapi.json` is stale, and CI fails when the generated TypeScript is. `fetchbox --print-openapi` prints the document without starting the server.

## Authentication

The API is protected by a single shared password. There are three ways to set it up, in order of precedence:

1. **From the environment or a file** (`FETCHBOX_PASSWORD`, or `--password-file /run/secrets/fetchbox_password`): at least 8 characters. It wins over everything and cannot be changed in the UI.
2. **In the browser, on first run** (the default when neither is set): the server starts locked and prints a one-time **setup code** to its log:

   ```text
   WARN fetchbox: first run: open the web UI and enter the setup code K7QM2XPD to create your account ...
   ```

   Open the UI (`docker compose logs fetchbox` shows the line in Docker), walk through the short setup wizard (language, look, password) and enter the code to create the account. The code guards a fresh server against whoever reaches it first; it is new on every start until the account exists, and wrong guesses count towards the login throttle. The password is stored as an Argon2id hash in the SQLite database and can be changed under *Settings*, which ends all other sessions.
3. **No authentication**, only on a fully trusted network: `--no-auth`.

**Change the password** under *Settings > Password* (current, new, new again). It signs out your other devices.

**Forgot it?** There is no e-mail reset: the server only keeps a hash, so you reset it where the server runs. This removes the stored password (downloads and settings stay); the server then starts like on the first run, with a setup code in its log, and you create a new account in the browser. The login screen shows the same steps under *Forgot your password?*.

```bash
# Docker Compose
docker compose stop fetchbox
docker compose run --rm fetchbox reset-password
docker compose up -d fetchbox
docker compose logs fetchbox      # the line with "setup code"

# Without Docker (same --data-dir as the server)
fetchbox --data-dir ./data reset-password
```

A password set with `FETCHBOX_PASSWORD` or `--password-file` is not stored by the server: change it there and restart.

Log in with `POST /api/auth/login {"password": "..."}`: the response sets an `HttpOnly` session cookie and also returns a token usable as `Authorization: Bearer <token>`. After 5 failed attempts logins are locked for up to a minute. Sessions are kept in memory (a restart logs everyone out). Serve it over HTTPS via a reverse proxy when exposed beyond the LAN (send `X-Forwarded-Proto: https` so the cookie gets `Secure`).

## Build

```bash
./scripts/build.sh          # web build, then cargo build --release -> target/release/fetchbox
docker compose up --build   # UI on http://localhost:8080, volumes ./data and ./downloads
```

## Upgrading from `dlmanager`

The project used to be called `dlmanager`. An existing installation keeps working:

- the database `dlmanager.db` in the data directory is renamed to `fetchbox.db` on the first start (nothing else to do; the account and the downloads stay),
- `DLM_PASSWORD` is still read (with a warning) when `FETCHBOX_PASSWORD` is not set; the theme, language, accent and list filters in the browser are carried over to the new storage keys,
- everybody has to sign in once more (the session cookie has a new name),
- in Docker the service and container are now called `fetchbox`: run `docker compose up -d --build --remove-orphans` to replace the old `dlmanager` container; `./data` and `./downloads` are reused.

## Layout

Workspace crates live in `crates/` (`core`, `http_engine`, `torrent_engine`, `storage`, `api`, `server`); package names are prefixed `fetchbox-` to avoid clashing with std `core`. The frontend is in `web/`.

## Dependency notes

`tailwindcss` + `@tailwindcss/vite` (build-time only: consistent design tokens and no hand-written CSS to maintain; the output is a few kB), `@fontsource-variable/inter` (the font files, so no CDN), `tokio` + `axum` (one async stack), `rust-embed` + `mime_guess` (embedded SPA), `rusqlite` bundled (lighter than sqlx, no system SQLite), `argon2` (Argon2id hash of the password created in the first-run setup; pure Rust, no system library, no RNG framework since the salt comes from `getrandom`), `rustix` (free disk space through `statvfs` without `unsafe`; it was already built as a dependency of `tempfile`), `clap` (CLI), `serde`/`serde_json`, `thiserror`/`anyhow`, `tracing`, `async-trait` (`Downloader` trait object).
