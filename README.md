# FetchBox

[![CI](https://github.com/fwitkowski17/WebDownloadManager/actions/workflows/ci.yml/badge.svg)](https://github.com/fwitkowski17/WebDownloadManager/actions/workflows/ci.yml)

A small, self-hosted download manager for a homelab, controlled from a web interface on a desktop or a phone. One Rust binary serves the API and the embedded web app, keeps a queue, and carries on when your browser is closed. No database server, no Node.js at runtime.

## Features

- **Downloads** with live progress (status, size, speed, ETA), pause and resume from the saved offset, retry, and several connections per file for speed.
- **Queue and limits**: a cap on simultaneous downloads, a default speed per download, a speed of its own for a single download, and a total speed limit (KB/s, MB/s, Kbit/s or Mbit/s). Changes apply live.
- **Download hours**: download only between two times of the day (a night data package, or a line that must stay free by day), in your time zone. A single download can start now regardless, or be set to begin at a chosen time.
- **Adding**: paste a link (there is a paste button) and see the file's size and name before you start. If the link is a directory index page (Apache, nginx and the like), pick which files to download.
- **Categories** (Photo, Music, Video, Program, Archive, Other), detected from the name or the server, changeable, with optional folders per category.
- **Disk aware**: a ring shows the free space, a download that does not fit is refused or held until there is room.
- **Statistics**: totals, success rate, data downloaded, average and peak speed, per category and for the last 14 days. They survive removing entries from the list.
- **Lists**: sort, filter, search, select several downloads, archive finished ones.
- **Phone friendly**: installable web app, light / dark / black (for OLED) themes and eight accent colours, [English and Polish](#languages).
- **Safe by default**: password protected (set in the browser on first run), optional blocking of private addresses, file names are sanitised and nothing is written outside the download folder.
- **Update notice**: tells you when a new release is out and what changed, with the steps to update. It can be switched off.

Planned: BitTorrent (magnet links and `.torrent` files). Status: usable, version 0.x.

## Install

| Method | Good for | |
| --- | --- | --- |
| [Docker Compose](#docker-compose) | any machine with Docker | builds the image from this repository |
| [Proxmox LXC](#proxmox-lxc) | a Proxmox VE host | one command, optional host folder for the downloads |
| [Release binary](#release-binary) | any Linux box | one file, run it yourself or under systemd |
| [From source](#from-source) | development | Rust and Node.js |

### Docker Compose

```bash
git clone https://github.com/fwitkowski17/WebDownloadManager.git fetchbox
cd fetchbox
docker compose up -d --build
docker compose logs fetchbox     # the line with the one-time "setup code"
```

Open <http://localhost:8080> and create your account with the setup code ([details](#signing-in)). The database goes to `./data` and the downloads to `./downloads`; change the `volumes:` in `docker-compose.yml` to put the downloads elsewhere (for example `/mnt/nas/downloads:/downloads`). The image is Debian 13 based and runs as the unprivileged user 65532 without a shell, so give it the two folders before the first start (otherwise Docker creates them for root and FetchBox cannot write to them):

```bash
mkdir -p data downloads && sudo chown 65532:65532 data downloads
```

### Proxmox LXC

On the Proxmox host, in the style of the [community-scripts](https://github.com/community-scripts/ProxmoxVE):

```bash
export COMMUNITY_SCRIPTS_URL=https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/deploy/proxmox
bash -c "$(curl -fsSL $COMMUNITY_SCRIPTS_URL/ct/fetchbox.sh)"
```

It creates a small Debian 13 container, installs the prebuilt binary from the latest [release](https://github.com/fwitkowski17/WebDownloadManager/releases) and can bind-mount a directory of the host as `/downloads`. See [deploy/proxmox/README.md](deploy/proxmox/README.md) for the options, the permissions of the mapped folder and updating.

### Release binary

Download `fetchbox-linux-x86_64.tar.gz` (or `-aarch64`) from the [Releases](https://github.com/fwitkowski17/WebDownloadManager/releases) page, unpack it and run:

```bash
./fetchbox --data-dir ./data --download-dir ./downloads --port 8080
```

The web app is inside the binary. It needs Debian 12 or newer, Ubuntu 22.04 or newer, or another Linux with a glibc that old. The container installer above contains a ready-made systemd unit you can copy ([`install/fetchbox-install.sh`](deploy/proxmox/install/fetchbox-install.sh)).

### From source

You need the Rust toolchain (version pinned by `rust-toolchain.toml`) and Node.js 22:

```bash
./scripts/build.sh     # builds the web app, then target/release/fetchbox
```

## Configuration

Everything day to day (connections, limits, category folders, theme, language, update check) is in **Settings** in the web app and applies at once. The rest is on the command line and in the environment:

| Option | Meaning |
| --- | --- |
| `--data-dir <dir>` | where the database lives (default `./data`) |
| `--download-dir <dir>` | where files are saved (default `./downloads`) |
| `--port <port>` | TCP port (default `8080`) |
| `--password-file <file>` | read the password from a file, e.g. a Docker secret |
| `--no-auth` | no password at all: only on a fully trusted network |
| `--block-private-urls` | refuse links to loopback, private and link-local addresses (SSRF protection); off by default so you can fetch from a NAS |
| `FETCHBOX_PASSWORD` | a fixed password (8+ characters) instead of one created in the browser |
| `FETCHBOX_UPDATE_URL` | use another releases endpoint for the update check (a mirror, a fork) |
| `--log-activity` / `FETCHBOX_LOG_ACTIVITY` | log every added, paused, finished and removed download (with its average speed), settings changes, the download hours and sign-ins; without it the console shows start-up information, warnings and errors |
| `RUST_LOG` | log level of everything else, e.g. `debug` (default `info`) |

Put a reverse proxy with HTTPS in front when it is reachable beyond your LAN, and send `X-Forwarded-Proto: https` so the session cookie gets `Secure`. The live updates use server-sent events: switch response buffering off for `/api/events` (the server already sends `X-Accel-Buffering: no`).

## Signing in

There is one shared password. In order of precedence:

1. **From the environment or a file** (`FETCHBOX_PASSWORD`, `--password-file`): it wins and cannot be changed in the app.
2. **In the browser on first run** (the default): the server starts locked and prints a one-time **setup code** to its log:

   ```text
   WARN fetchbox: first run: open the web UI and enter the setup code K7QM2XPD to create your account ...
   ```

   Open the app, walk through the short setup (language, look, password) and enter the code. The code guards a fresh server against whoever reaches it first and is new on every start until the account exists. The password is stored as an Argon2id hash and can be changed under *Settings > Password*, which signs out your other devices.
3. **No authentication**: `--no-auth`.

**Forgot the password?** There is no e-mail reset, because the server only keeps a hash. Reset it where the server runs; downloads and settings stay, and you create a new account with a new setup code:

```bash
# Docker Compose
docker compose stop fetchbox
docker compose run --rm fetchbox reset-password
docker compose up -d fetchbox
docker compose logs fetchbox      # the line with "setup code"

# Without Docker (same --data-dir as the server)
fetchbox --data-dir ./data reset-password
```

A password from `FETCHBOX_PASSWORD` or `--password-file` is not stored by the server: change it there and restart. For scripts: `POST /api/auth/login {"password": "..."}` sets a session cookie and returns a token usable as `Authorization: Bearer <token>`; after 5 failed attempts logins are locked for up to a minute.

## Updating

FetchBox looks at the GitHub releases now and then (only a request for the list of releases is sent; **Settings > About** switches it off) and tells you in the app when there is a newer version, with the changes since yours and these steps:

- **Docker Compose**: `git pull && docker compose up -d --build fetchbox`
- **Proxmox LXC**: run `update` in the container's console
- **Release binary**: replace the file and restart it
- **From source**: `git pull && ./scripts/build.sh`

Your data stays and unfinished downloads continue after the restart. The app does not update itself (inside a container that is neither possible nor safe). 
<details>
<summary>Coming from the old name <code>dlmanager</code>?</summary>

An existing installation keeps working:

- the database `dlmanager.db` in the data directory is renamed to `fetchbox.db` on the first start, and the account and downloads stay;
- `DLM_PASSWORD` is still read (with a warning) when `FETCHBOX_PASSWORD` is not set, and the theme, language, accent and list filters in the browser are carried over;
- everybody has to sign in once more (the session cookie has a new name);
- in Docker the service and container are now called `fetchbox`: run `docker compose up -d --build --remove-orphans` to replace the old `dlmanager` container; `./data` and `./downloads` are reused.

</details>

## Languages

The interface is translated without a library, and a new language is one file and one line. The status of each:

| Language | Code | Status | Notes |
| --- | --- | --- | --- |
| Polski | `pl` | Complete, the reference | written first, by the maintainer; new texts are made in Polish |
| English | `en` | Complete | where the keys are declared in the code (`en.ts`); the other languages follow them |

"Complete" is enforced by the build: a language that lacks a key does not compile, and the tests check placeholders and plural forms. Your language is welcome: [how to add one](CONTRIBUTING.md#adding-a-language) takes an evening for the roughly 400 messages; start from the Polish file and keep its meaning. Messages that come from the backend (for example why a download could not be added) are shown as the server sent them and are not translated yet.

## Documentation

| | |
| --- | --- |
| [docs/SPEC.md](docs/SPEC.md) | the specification: architecture and every design decision |
| [docs/openapi.json](docs/openapi.json) | the HTTP API (OpenAPI 3.1), also served at `/api/openapi.json` |
| [CONTRIBUTING.md](CONTRIBUTING.md) | how to develop, test, translate and release |
| [deploy/proxmox/README.md](deploy/proxmox/README.md) | the Proxmox LXC installer |

## Contributing

Bug reports, ideas, translations and code are all welcome. Open an [issue](https://github.com/fwitkowski17/WebDownloadManager/issues) first for anything bigger than a small fix. The short version of sending a change:

```bash
cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads   # backend
cd web && npm install && npm run dev                                           # web app on :5173, proxies /api to :8080
```

Before a pull request run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and, in `web/`, `npm run check` and `npm test`; CI runs the same. Everything else (layout of the code, conventions, translations, releases) is in [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT, as declared in `Cargo.toml`; see [LICENSE](LICENSE).
