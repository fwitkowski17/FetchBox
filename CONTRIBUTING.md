# Contributing to FetchBox

Thank you for helping. Bug reports, ideas, translations and code are all welcome. For anything bigger than a small fix, please open an [issue](https://github.com/fwitkowski17/WebDownloadManager/issues) first so we can agree on the approach before you spend the time.

The specification in [docs/SPEC.md](docs/SPEC.md) is the source of truth for how FetchBox is meant to behave and why. Read the part that concerns your change; when a change alters behaviour, update the specification in the same pull request.

## Set up

You need:

- **Rust**, the stable toolchain (`rust-toolchain.toml` selects it; install it with [rustup](https://rustup.rs)),
- **Node.js 22** with npm,
- optionally Docker, to try the image.

```bash
git clone https://github.com/fwitkowski17/WebDownloadManager.git
cd WebDownloadManager

# backend (http://127.0.0.1:8080)
cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads

# frontend with hot reload (http://localhost:5173, /api is proxied to :8080)
cd web && npm install && npm run dev
```

On first run the server prints a setup code ([see the README](README.md#-signing-in)); `--no-auth` skips the password while you develop.

## Before you send a pull request

CI runs exactly this, so run it locally first:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && npm run check && npm test && npm run build
```

- Keep pull requests small and focused, with small commits. Commit messages are in English, in the form `type(scope): what and why` as in the history (`feat(api): ...`, `fix(web): ...`, `docs: ...`, `build(docker): ...`).
- Add tests with new logic: the scheduler, the rate limiter, category detection, segment planning, parsers and so on are tested, and so should new ones be. UI behaviour is checked in a real browser by hand; say in the pull request what you tried.
- A change to the HTTP API needs the generated files refreshed (see [below](#the-http-api-and-generated-types)).
- A change in behaviour needs `docs/SPEC.md` (and the README, if users see it) updated.
- Do not add a dependency without a line of justification in the pull request: the aim is a light runtime.

## Where things are

```text
crates/
  core/            models, categories, the Downloader trait, scheduler, rate limiter (no HTTP, no SQL, no axum)
  http_engine/     segmented HTTP download with Range, pause/resume, probing, directory-index parsing
  torrent_engine/  empty stub for BitTorrent later (librqbit)
  storage/         SQLite repositories and migrations
  api/             axum routes, SSE, authentication, the OpenAPI document
  server/          the binary `fetchbox`: wires everything, the download manager, update check, embedded frontend, CLI
web/               Svelte 5 + Vite + TypeScript frontend, embedded into the binary at build time
deploy/install.sh  installer for a Linux host with systemd (run `shellcheck` on it)
packaging/         DEB and RPM: nfpm description, systemd unit, maintainer scripts, smoke test
deploy/proxmox/    the Proxmox LXC installer
docs/              SPEC.md and the generated openapi.json
scripts/           build.sh, gen-api.sh
```

Crate packages are named `fetchbox-*` to avoid clashing with the standard `core`. Inside the frontend: `web/src/lib/{api,components,stores,util,i18n}`, and the pages in `web/src/routes`.

### Rules of the code

- **Rust**: stable, no `unwrap()` or `expect()` outside tests and the start-up in `main`. `thiserror` in libraries, `anyhow` in the `server` binary, `tracing` for logs and never `println!`.
- **Hot path** (network reads, disk writes, rate limiting) has no blocking calls and no SQLite; progress is persisted in batches through a channel.
- **Safety**: sanitise file names, validate URLs, never write outside the download directory.
- **Crates keep their jobs**: for example `core` knows nothing about HTTP or SQL.
- **Frontend**: TypeScript strict, no `any`, small components, stores in `src/lib/stores`. No UI framework; Tailwind CSS 4 at build time. Tests find elements by `data-*` attributes and ARIA roles, never by style classes.
- One process, one binary: no Node.js or other runtime in the production image, and no second deployable service.

### The HTTP API and generated types

The API is described by an OpenAPI 3.1 document generated from the Rust code (`utoipa`), served at `/api/openapi.json` and committed as `docs/openapi.json`. The frontend types in `web/src/lib/api/schema.d.ts` are generated from it and are never edited by hand. After changing anything in the API:

```bash
./scripts/gen-api.sh   # rewrites docs/openapi.json and web/src/lib/api/schema.d.ts
```

A test fails when `docs/openapi.json` is stale, and CI fails when the generated TypeScript is. `fetchbox --print-openapi` prints the document without starting the server.

## Adding a language

Translations live in `web/src/lib/i18n/`, one typed dictionary per language, no library. To add one, say German:

1. Copy `pl.ts` (the reference) to `de.ts` and translate the values. Keep the `{placeholders}` as they are. A message that depends on a count is an object with the plural forms of that language (`one`, `few`, `many`, `other`, whatever it needs; see `downloads.cleared`).
2. Add one line to `locales.ts`: `{ code: 'de', name: 'Deutsch', load: async () => (await import('./de')).default }`.
3. Run `npm run check` and `npm test`.

That is all: the language appears in Settings, is loaded on demand as its own small chunk, and is picked automatically when it matches the browser language. Polish is the reference translation (new texts are written in it first); `en.ts` is where the keys are declared in code, and the compiler rejects a missing or misspelled key, and the tests reject a changed placeholder or a missing plural form. Then add the language to the table in the README.

## Frontend notes

Svelte 5 with runes, Vite, Tailwind CSS 4, no component framework. The app is about 70 kB of gzipped JavaScript and 10 kB of CSS, plus the font. Tailwind 4 needs a reasonably recent browser (Safari 16.4+, Chrome 111+, Firefox 128+).

- **Colours** are CSS variables defined per theme in `web/src/app.css` and exposed to Tailwind through `@theme inline`, so classes such as `bg-surface`, `text-muted` or `border-border` follow light, dark and black without any `dark:` variants. Shared controls (`.btn`, `.input`, `.select`, `.field`) are defined once in the same file.
- **Accent colours** are one hue on top of the theme: lightness and saturation come from the theme (`--accent-l`, `--accent-c`, which keeps text on the accent readable), the hue is the user's choice (`web/src/lib/util/accent.ts`, a table of ids, hues and saturation factors). A test computes the contrast of every accent in every theme from the real CSS and fails below WCAG AA. Adding an accent is one line in that table plus its name in each language file.
- **The font** is Inter (variable), bundled so nothing is fetched from a CDN and it works on an isolated LAN. Only the `latin` and `latin-ext` subsets are included, and a browser downloads one only when a character from it is on the page.
- **Motion** is short and functional and respects `prefers-reduced-motion` (see the specification, 6.6).

## Dependencies

Each one earns its place: `tokio` + `axum` (one async stack), `reqwest` with `rustls` (no OpenSSL), `rusqlite` bundled (lighter than `sqlx`, no system SQLite), `rust-embed` + `mime_guess` (the embedded web app), `argon2` (the password hash; pure Rust, the salt comes from `getrandom`), `rustix` (free disk space through `statvfs` without `unsafe`), `jiff` (time zones for the download hours, tz database bundled), `clap`, `serde`, `thiserror`/`anyhow`, `tracing`, `async-trait`, `utoipa` (derive only). On the frontend: `tailwindcss` (build time only) and `@fontsource-variable/inter` (the font files).

## Releasing

Maintainers: a release is a version change that reaches `main`.

```bash
scripts/bump-version.sh 0.7.0     # Cargo.toml, Cargo.lock, web/package.json and its lock, docs/openapi.json
git checkout -b release-0.7.0 && git commit -am "Release 0.7.0" && git push -u origin release-0.7.0
# open a pull request and merge it
```

When the merge changes `Cargo.toml` and the version has no tag yet, `.github/workflows/tag.yml` tags it (`v0.7.0`) and starts the release workflow on that tag. Pushing the tag yourself (`git tag v0.7.0 && git push origin v0.7.0`) works too, and so does running the "Release" workflow by hand on the tag's ref.

`.github/workflows/release.yml` checks that the tag equals the version, builds the web app and the binary for x86_64 and aarch64 Linux, wraps them in DEB and RPM packages (`packaging/`), installs each package in a clean Debian 12 or Fedora container as a test and publishes a GitHub Release with the archives, packages, checksums and generated notes. A tag with a dash (`v0.7.0-rc.1`) makes a pre-release, which the update check announces only to users who switched on pre-releases, and which the installers' update commands skip. The Proxmox installer and the update notice in the app read that release; the notes are what users see in the update dialog, so read them before they go out. See [deploy/proxmox/README.md](deploy/proxmox/README.md#releasing) for the details.

## Licence

By contributing you agree that your work is released under the [MIT licence](LICENSE) of the project.
