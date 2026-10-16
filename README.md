# FetchBox

[![CI](https://github.com/fwitkowski17/WebDownloadManager/actions/workflows/ci.yml/badge.svg)](https://github.com/fwitkowski17/WebDownloadManager/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fwitkowski17/WebDownloadManager?include_prereleases&sort=semver)](https://github.com/fwitkowski17/WebDownloadManager/releases)

A small, self-hosted download manager for a homelab, controlled from a web interface on a desktop or a phone. One Rust binary serves the API and the embedded web app, keeps a queue, and carries on when your browser is closed. No database server, no Node.js at runtime.

## ✨ Features

- ⬇️ **Downloads** with live progress (status, size, speed, ETA), pause and resume from the saved offset, retry, and several connections per file for speed.
- 🚦 **Queue and limits**: a cap on simultaneous downloads, a default speed per download, a speed of its own for a single download, and a total speed limit (KB/s, MB/s, Kbit/s or Mbit/s). Changes apply live.
- 🌙 **Download hours**: download only between two times of the day (a night data package, or a line that must stay free by day), in your time zone. A single download can start now regardless, or be set to begin at a chosen time.
- 🔗 **Adding**: paste a link (there is a paste button) and see the file's size and name before you start. If the link is a directory index page (Apache, nginx and the like), pick which files to download.
- 🗂️ **Categories** (Photo, Music, Video, Program, Archive, Other), detected from the name or the server, changeable, with optional folders per category.
- 💾 **Disk aware**: a ring shows the free space, a download that does not fit is refused or held until there is room.
- 📊 **Statistics**: totals, success rate, data downloaded, average and peak speed, per category and for the last 14 days. They survive removing entries from the list.
- 🔎 **Lists**: sort, filter, search, select several downloads, archive finished ones.
- 📱 **Phone friendly**: installable web app, light / dark / black (for OLED) themes and eight accent colours, [English and Polish](#-languages).
- 🔒 **Safe by default**: password protected (set in the browser on first run), optional blocking of private addresses, file names are sanitised and nothing is written outside the download folder.
- 🔔 **Update notice**: tells you when a new release is out and what changed, with the steps to update. It can be switched off.
- 🪶 **Light**: one Rust binary of about 10 MB, around 64 MB of RAM when idle, no database server and no Node.js at runtime.

> [!NOTE]
> Status: usable, version 0.x. What comes next is in the [roadmap](#-roadmap).

## 📋 Requirements

FetchBox is small, and a download manager is mostly waiting for the network and the disk, so almost any always-on machine will do: a mini PC, a NAS with Docker, a Raspberry Pi, a VM or a Proxmox container. What it asks for:

| | Minimum | Recommended |
| --- | --- | --- |
| 🧠 **CPU** | 1 core, 64-bit: x86_64 or ARM64 (Raspberry Pi 3 and newer with a 64-bit OS) | 2 cores; 4 cores for a gigabit line with many files at once |
| 🧮 **RAM** | 256 MB free (the program itself needs about 64 MB when idle) | 512 MB to 1 GB, more when the list holds many thousands of entries |
| 🐧 **System** | a 64-bit Linux with glibc 2.36 or newer and systemd (or Docker) | **Debian 13 or 12** (what the release is built and tested on), or Ubuntu 22.04 / 24.04 LTS, Raspberry Pi OS 64-bit, Fedora |
| 🗄️ **Disk for the program** | 100 MB (the binary, and a database that stays small even after thousands of downloads) | the system disk; on a Raspberry Pi an SSD or a good SD card |
| 📥 **Disk for downloads** | free space for the files that are in progress at the same time (the finished ones stay) | **a separate disk, partition or network share**. An SSD is the recommended choice, but a CMR hard disk is entirely enough, especially for lines up to 1 Gbit/s. ⚠️ Not a memory card, and not an SMR disk if your line is fast (see [below](#-disk-speed-and-memory-cards)) |
| 🌐 **Browser** | any current Chrome, Edge, Firefox or Safari | the same, over HTTPS (installing the web app and the paste button need a secure context) |

A few notes behind those numbers:

- **A separate disk for downloads** is the one recommendation that matters most. A full download disk then cannot take the system or the database down with it, FetchBox shows exactly the space that is meant for downloads, you can size, replace or back it up on its own, and on a Raspberry Pi it keeps the heavy writing off the SD card. Mount it, make the folder writable for the service's user, and point `FETCHBOX_DOWNLOAD_DIR` or `--download-dir` at it (in a Proxmox container map a host folder, see [Proxmox LXC](#proxmox-lxc)).
- **File system**: ext4, XFS or Btrfs are the best fit. A file is created at its full size before the first byte arrives, which costs nothing where files can be sparse (ext4, XFS, Btrfs, APFS); on exFAT, FAT or some network shares the whole size may be allocated up front, and FAT32 cannot hold files over 4 GB at all.
- **CPU**: the work is network and disk. One core is enough for a few downloads at 100 Mbit/s; a gigabit line with HTTPS and many connections per file is where a second and third core help. The limits you set in Settings keep it below whatever the machine can do.
- **Operating systems** older than Debian 12 or Ubuntu 22.04, 32-bit systems (including 32-bit Raspberry Pi OS) and musl systems such as Alpine are not supported by the release binaries.

### 💽 Disk speed and memory cards

> [!WARNING]
> **Downloading to a memory card (SD, microSD) is NOT recommended, above all when the system is on that card.** A download manager writes all day, a card has little endurance and slow writes, and when it wears out, fills up or is cut off in the middle of a write, it takes the operating system with it. On a Raspberry Pi keep the system on the card if you must, but put the downloads on an SSD or a hard disk on USB. The same goes for USB sticks.

**What happens when the line is faster than the disk.** FetchBox reads from the network only as fast as it can hand the data to the disk, so nothing piles up in memory (a few MB per connection) and the download does not fail: the server is simply slowed down through the connection. The data first goes into the operating system's write cache, so the first seconds, or the first gigabytes on a machine with a lot of free RAM, can run at line speed and look faster than the disk really is; once the cache is full the speed falls to what the disk sustains. The speed shown in the app is the speed at which data is accepted for writing.

- **SMR hard disks** (shingled, common in cheap 2.5" and some "desktop" drives) take short bursts quickly into a small cache area and then rewrite whole bands of tracks in the background. A long download of many gigabytes can drop to a fraction of the advertised speed, and writes to several places of the file at once make it worse. If your line is faster than about 100 MB/s, or you download huge files, prefer an SSD or a CMR drive (the data sheet says which; "SMR" is often not stated, the model number tells).
- **An SSD is the recommended choice, but a CMR hard disk is entirely enough** (especially for lines up to 1 Gbit/s), and it is rarely the bottleneck; a RAID array even less. A conventional (CMR) 3.5" disk sustains roughly 100 to 250 MB/s of sequential writes, which covers a 1 Gbit/s line (about 120 MB/s); a 2.5" one is nearer the lower end and may be the limit at 1 Gbit/s. Striped arrays (RAID 0, 5, 6, 10, ZFS or Btrfs over several disks) add the speed of their disks together, so even a 2.5 or 10 Gbit/s line is within reach, while RAID 1 writes at the speed of one disk. Downloads are large sequential writes, which arrays handle well. The one thing to check is that the drives really are CMR: the name on the box does not always tell, and not every maker states it in the product name, so look up the exact model number in the maker's CMR/SMR list or its data sheet before you rely on it. A RAID is not a backup, and arrays built from SMR drives are known for painfully slow rebuilds.
- **On any hard disk, fewer connections per download is gentler**: several connections write to different places of the same file, so the head has to jump between them. Try 1 to 2 in **Settings** (connections per download) and keep the number of simultaneous downloads low. A speed limit under what the disk sustains gives a steady transfer instead of bursts and stalls.
- **A power cut or a kernel crash.** The data you have downloaded is forced to the disk every couple of seconds, and the point a download resumes from is stored only after that, so after a power cut a download continues from what is really in the file (at most a few seconds are fetched again). A finished file is synced before it gets its real name. This costs a little speed on a slow disk, because the program waits for the disk instead of hiding behind the system's cache for long.

> [!NOTE]
> How FetchBox behaves when the line is faster than the disk, on slow and SMR drives, and the syncing after a power cut are new and **not tested thoroughly yet**. If you see something different, please open an [issue](https://github.com/fwitkowski17/WebDownloadManager/issues).

## 📦 Install

| Method | Good for | |
| --- | --- | --- |
| 🐳 [Docker Compose](#docker-compose) | any machine with Docker | builds the image from this repository |
| 🖥️ [Proxmox LXC](#proxmox-lxc) | a Proxmox VE host | one command, optional host folder for the downloads (installs the DEB) |
| 📦 [DEB / RPM package](#deb--rpm-package) | Debian, Ubuntu, Raspberry Pi OS, Fedora | a package with a systemd service, x86_64 and ARM |
| 🐧 [Linux host (systemd)](#linux-host-systemd) | any other Linux box with systemd | one command: installs the binary as a service |
| 🧱 [Release binary](#release-binary) | any Linux box | one file, run it yourself |
| 🛠️ [From source](#from-source) | development | Rust and Node.js |

### Docker Compose

```bash
git clone https://github.com/fwitkowski17/WebDownloadManager.git fetchbox
cd fetchbox
docker compose up -d --build
docker compose logs fetchbox     # the line with the one-time "setup code"
```

Open <http://localhost:8080> and create your account with the setup code ([details](#-signing-in)). The database goes to `./data` and the downloads to `./downloads`; change the `volumes:` in `docker-compose.yml` to put the downloads elsewhere (for example `/mnt/nas/downloads:/downloads`). The image is Debian 13 based and runs as the unprivileged user 65532 without a shell, so give it the two folders before the first start (otherwise Docker creates them for root and FetchBox cannot write to them):

```bash
mkdir -p data downloads && sudo chown 65532:65532 data downloads
```

### Proxmox LXC

On the Proxmox host, in the style of the [community-scripts](https://github.com/community-scripts/ProxmoxVE):

```bash
export COMMUNITY_SCRIPTS_URL=https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/deploy/proxmox
bash -c "$(curl -fsSL $COMMUNITY_SCRIPTS_URL/ct/fetchbox.sh)"
```

It creates a small Debian 13 container, installs the `.deb` package from the latest [release](https://github.com/fwitkowski17/WebDownloadManager/releases) and can bind-mount a directory of the host as `/downloads`. See [deploy/proxmox/README.md](deploy/proxmox/README.md) for the options, the permissions of the mapped folder and updating.

### DEB / RPM package

Every [release](https://github.com/fwitkowski17/WebDownloadManager/releases) has a `.deb` (`amd64`, `arm64`) and an `.rpm` (`x86_64`, `aarch64`). Installing one adds the program, a systemd service that starts at boot, the `fetchbox` user and the settings file, and starts it:

```bash
# Debian, Ubuntu, Raspberry Pi OS
url="$(curl -fsSL https://api.github.com/repos/fwitkowski17/WebDownloadManager/releases/latest | grep -o "\"browser_download_url\": *\"[^\"]*_$(dpkg --print-architecture)\.deb\"" | cut -d'"' -f4)"
curl -fsSLO "$url" && sudo apt install "./$(basename "$url")"

# Fedora and other RPM systems (glibc 2.36 or newer; RHEL 9 is too old)
url="$(curl -fsSL https://api.github.com/repos/fwitkowski17/WebDownloadManager/releases/latest | grep -o "\"browser_download_url\": *\"[^\"]*\.$(uname -m)\.rpm\"" | cut -d'"' -f4)"
curl -fsSLO "$url" && sudo dnf install "./$(basename "$url")"
```

(Or download the file from the Releases page and give it to `apt install` / `dnf install`. Each file has a `.sha256` next to it.) The installation prints the address, `http://<host>:8080`, and the one-time **setup code** for creating the account (later: `journalctl -u fetchbox | grep 'setup code'`).

| What | Where |
| --- | --- |
| program | `/usr/bin/fetchbox` |
| service | `systemctl status fetchbox`, logs: `journalctl -u fetchbox -f` |
| settings | `/etc/fetchbox/fetchbox.env` (`FETCHBOX_PASSWORD`, `FETCHBOX_PORT`, `FETCHBOX_DOWNLOAD_DIR`...), then `systemctl restart fetchbox`; an upgrade keeps your edits |
| database | `/var/lib/fetchbox` |
| downloads | `/var/lib/fetchbox/downloads`, or the folder you set as `FETCHBOX_DOWNLOAD_DIR` (it must be writable for the user `fetchbox`) |

To update, install the newer file the same way (the service is restarted).

> [!NOTE]
> `apt` and `dnf` do not know about GitHub releases, so `apt upgrade` does not update FetchBox.

Removing the package (`apt remove fetchbox`, `dnf remove fetchbox`) stops the service but keeps the database, the downloads and the user; delete `/var/lib/fetchbox` (and `apt purge` removes the settings) to wipe it. The packages are tested on Debian 12 and Fedora on both CPUs by the release workflow.

### Linux host (systemd)

```bash
curl -fsSL https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/deploy/install.sh | sudo bash
```

The script downloads the latest [release](https://github.com/fwitkowski17/WebDownloadManager/releases) for your CPU (x86_64 or aarch64), checks its checksum, creates a `fetchbox` user, writes `/etc/systemd/system/fetchbox.service`, starts the service and prints the address and the one-time setup code. Nothing is compiled. Options go after `bash -s --`, for example `... | sudo bash -s -- install --port 9000 --download-dir /mnt/nas/downloads`; `--user NAME` runs the service as an existing user (the owner of a NAS folder) and `--version X.Y.Z` picks a release.

| What | Where |
| --- | --- |
| program | `/usr/local/bin/fetchbox` |
| settings (`FETCHBOX_PASSWORD`, `FETCHBOX_LOG_ACTIVITY`...) | `/etc/fetchbox/fetchbox.env`, then `systemctl restart fetchbox` |
| database | `/var/lib/fetchbox` |
| downloads | `/var/lib/fetchbox/downloads` unless `--download-dir` says otherwise |
| logs | `journalctl -u fetchbox -f` |

Download the script first (`curl -fsSLO .../deploy/install.sh`) if you want to read it before running it. Later:

```bash
sudo ./install.sh update               # new program, the same database, settings and unit
sudo ./install.sh uninstall            # removes the service and the program, keeps data and downloads
sudo ./install.sh uninstall --purge    # also the database, the settings and the service user (never the downloads)
```

It needs root, systemd, `curl` or `wget`, `tar` and `sha256sum`; `./install.sh help` lists everything.

### Release binary

Download `fetchbox-linux-x86_64.tar.gz` (or `-aarch64`) from the [Releases](https://github.com/fwitkowski17/WebDownloadManager/releases) page, unpack it and run:

```bash
./fetchbox --data-dir ./data --download-dir ./downloads --port 8080
```

The web app is inside the binary. It needs Debian 12 or newer, Ubuntu 22.04 or newer, or another Linux with a glibc that old. To keep it running, use the package or the installer above (they write the systemd unit for you).

### From source

You need the Rust toolchain (version pinned by `rust-toolchain.toml`) and Node.js 22:

```bash
./scripts/build.sh     # builds the web app, then target/release/fetchbox
```

## ⚙️ Configuration

Everything day to day (connections, limits, category folders, theme, language, update check) is in **Settings** in the web app and applies at once. The rest is on the command line and in the environment:

| Option | Meaning |
| --- | --- |
| `--data-dir <dir>` / `FETCHBOX_DATA_DIR` | where the database lives (default `./data`) |
| `--download-dir <dir>` / `FETCHBOX_DOWNLOAD_DIR` | where files are saved (default `./downloads`) |
| `--port <port>` / `FETCHBOX_PORT` | TCP port (default `8080`) |
| `--password-file <file>` | read the password from a file, e.g. a Docker secret |
| `--no-auth` | no password at all: see the caution below |
| `--block-private-urls` / `FETCHBOX_BLOCK_PRIVATE_URLS` | refuse links to loopback, private and link-local addresses (SSRF protection); off by default so you can fetch from a NAS |
| `FETCHBOX_PASSWORD` | a fixed password (8+ characters) instead of one created in the browser |
| `FETCHBOX_UPDATE_URL` | use another releases endpoint for the update check (a mirror, a fork) |
| `--log-activity` / `FETCHBOX_LOG_ACTIVITY` | log every added, paused, finished and removed download (with its average speed), settings changes, the download hours and sign-ins; without it the console shows start-up information, warnings and errors |
| `RUST_LOG` | log level of everything else, e.g. `debug` (default `info`) |

> [!CAUTION]
> `--no-auth` lets **anyone who can reach the port** add and delete downloads and write files into your download folder. Use it only on a network you fully trust, never on one that is reachable from the internet.

> [!IMPORTANT]
> Put a reverse proxy with HTTPS in front when FetchBox is reachable beyond your LAN, and send `X-Forwarded-Proto: https` so the session cookie gets `Secure`.

The live updates use server-sent events: switch response buffering off for `/api/events` (the server already sends `X-Accel-Buffering: no`).

## 🔑 Signing in

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

## 🔄 Updating

FetchBox looks at the GitHub releases now and then (only a request for the list of releases is sent; **Settings > About** switches it off) and tells you in the app when there is a newer version, with the changes since yours and these steps. Pre-releases (`0.7.0-rc.1`) are left out unless you switch on *Also announce pre-releases* in the same card:

- **Docker Compose**: `git pull && docker compose up -d --build fetchbox`
- **Proxmox LXC**: run `update` in the container's console
- **DEB / RPM package**: install the newer file (`apt install ./fetchbox_<version>_<arch>.deb`, `dnf install ./fetchbox-<version>-1.<arch>.rpm`)
- **Linux host script**: `sudo ./install.sh update`
- **Release binary**: replace the file and restart it
- **From source**: `git pull && ./scripts/build.sh`

Your data stays and unfinished downloads continue after the restart.

> [!NOTE]
> The app does not update itself (inside a container that is neither possible nor safe). The update commands of the installers (`update` in the Proxmox container, `install.sh update` without `--version`) install only stable versions; to try a pre-release, install it from its release page (`install.sh update --version 0.7.0-rc.1` also works).

## 🌍 Languages

The interface is translated without a library, and a new language is one file and one line. The status of each:

| Language | Code | Status | Notes |
| --- | --- | --- | --- |
| Polski | `pl` | Complete, the reference | written first, by the maintainer; new texts are made in Polish |
| English | `en` | Complete | where the keys are declared in the code (`en.ts`); the other languages follow them |

"Complete" is enforced by the build: a language that lacks a key does not compile, and the tests check placeholders and plural forms. Your language is welcome: [how to add one](CONTRIBUTING.md#adding-a-language) takes an evening for the roughly 400 messages; start from the Polish file and keep its meaning. Messages that come from the backend (for example why a download could not be added) are shown as the server sent them and are not translated yet.

## 🚀 Roadmap

Ideas in rough order, with no promised dates. The ones marked 🧭 are being designed.

- ✅ **Checksum verification**: give a download its expected SHA-256 (or MD5, SHA-1, SHA-512), or a link to a checksum file next to it, and see whether the finished file matches; a mismatch is reported instead of silently kept.
- 🧲 **BitTorrent**: magnet links and `.torrent` files through [`librqbit`](https://github.com/ikatson/rqbit), behind the same queue, limits, hours and statistics as the other downloads. The `torrent_engine` crate is already in place as a stub.
- 📈 **Metrics for Prometheus and VictoriaMetrics**: a `/metrics` endpoint in the Prometheus text format (downloads by state, queue length, current and total speed, bytes fetched, disk space), which both scrape as they are, plus an example Grafana dashboard. Pushing to VictoriaMetrics may follow.
- 🧩 **Plugins** 🧭: an extension system, with a place for it in the web app (a tab above Settings, or a part of Settings): hooks on events such as added, finished or failed (notifications, moving or renaming files, calling a webhook) and, later, extra sources. It is designed first so that an extension cannot take down the downloads.
- 📬 **A package repository**: `apt` and `dnf` pick up new releases by themselves, instead of installing a downloaded file.
- 🏔️ **A static (musl) build** for Alpine and very small containers.
- 🌐 **More languages**: [one file and one line each](CONTRIBUTING.md#adding-a-language).

> [!TIP]
> Wishes and bug reports go into the [issues](https://github.com/fwitkowski17/WebDownloadManager/issues).

## 📚 Documentation

| | |
| --- | --- |
| [docs/SPEC.md](docs/SPEC.md) | the specification: architecture and every design decision |
| [docs/openapi.json](docs/openapi.json) | the HTTP API (OpenAPI 3.1), also served at `/api/openapi.json` |
| [CONTRIBUTING.md](CONTRIBUTING.md) | how to develop, test, translate and release |
| [deploy/install.sh](deploy/install.sh) | the installer for a Linux host with systemd |
| [deploy/proxmox/README.md](deploy/proxmox/README.md) | the Proxmox LXC installer |

## 🤝 Contributing

Bug reports, ideas, translations and code are all welcome. Open an [issue](https://github.com/fwitkowski17/WebDownloadManager/issues) first for anything bigger than a small fix. The short version of sending a change:

```bash
cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads   # backend
cd web && npm install && npm run dev                                           # web app on :5173, proxies /api to :8080
```

Before a pull request run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and, in `web/`, `npm run check` and `npm test`; CI runs the same. Everything else (layout of the code, conventions, translations, releases) is in [CONTRIBUTING.md](CONTRIBUTING.md).

## 📄 License

MIT, as declared in `Cargo.toml`; see [LICENSE](LICENSE).
