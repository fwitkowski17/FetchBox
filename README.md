# FetchBox

[![CI](https://github.com/fwitkowski17/WebDownloadManager/actions/workflows/ci.yml/badge.svg)](https://github.com/fwitkowski17/WebDownloadManager/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fwitkowski17/WebDownloadManager?include_prereleases&sort=semver)](https://github.com/fwitkowski17/WebDownloadManager/releases)

A small, self-hosted download manager for a homelab, controlled from a web interface on a desktop or a phone. One Rust binary serves the API and the embedded web app, keeps a queue, and carries on when your browser is closed. No database server, no Node.js at runtime.

## ✨ Features

- 🌍 **HTTP, HTTPS, FTP, FTPS and SFTP**: one list, queue and set of limits for all of them, with logins for the servers that need one. [Details](#-sources-and-logins).
- 🧲 **Torrents**: magnet links and `.torrent` files on a page of their own, with the statuses of a BitTorrent client (fetching metadata, checking files, connecting to peers, downloading, sending...) and, at the bottom of the page, the details of the torrent you click: peers with their speeds, files, a speed chart. [Details](#-torrents).
- ⬇️ **Downloads** with live progress (status, size, speed, ETA), pause and resume from the saved offset, retry, and several connections per file for speed.
- 🚦 **Queue and limits**: a cap on simultaneous downloads, a default speed per download, a speed of its own for a single download, and a total speed limit (KB/s, MB/s, Kbit/s or Mbit/s). Changes apply live.
- 🌙 **Download hours**: download only between two times of the day (a night data package, or a line that must stay free by day), in your time zone. A single download can start now regardless, or be set to begin at a chosen time.
- ✅ **Checksum verification**: tick *Verify the checksum* when adding, pick the type (MD5, SHA-1, SHA-256, SHA-512; recognised from the pasted text) and paste it. The finished file is checked in the background (the card says *Verifying* and shows how far it has got) and then gets a green tick. A file that does not match lands in the failed ones, where you can download it again, keep it anyway, or delete it together with the file; a file you keep is *Completed* with a warning icon beside it (its tooltip shows the two checksums).
- 🔗 **Adding**: paste a link (there is a paste button) and see the file's size and name before you start. If the link is a folder (a directory index page of Apache or nginx, or an FTP or SFTP folder), pick which files to download.
- 🗂️ **Categories** (Photo, Music, Video, Program, Archive, Other), detected from the name or the server, changeable, with optional folders per category.
- 💾 **Disk aware**: a ring shows the free space, a download that does not fit is refused or held until there is room.
- 🗄️ **Several disks**: mount more than one disk in the download folder and choose which one new downloads go to (asked on first run, changed in Settings, with the capacity of each), or pick a disk for a single download when you add it. [Details](#-several-disks).
- 📊 **Statistics**: totals, success rate, data downloaded, average and peak speed, per category and for the last 14 days. They survive removing entries from the list.
- 🔎 **Lists**: sort, filter, search, select several downloads, archive finished ones. Show them as cards or as a compact table (the choice is remembered), where a click on a column header sorts by it.
- 📱 **Phone friendly**: installable web app, light / dark / black (for OLED) themes and eight accent colours, [English and Polish](#-languages).
- 🔒 **Safe by default**: password protected (set in the browser on first run), optional blocking of private addresses, file names are sanitised and nothing is written outside the download folder. An SSH server is only used after you have compared its fingerprint and trusted it.
- 🔔 **Update notice**: tells you when a new release is out and what changed, with the steps to update. It can be switched off.
- 🪶 **Light**: one Rust binary of about 24 MB (BitTorrent included), around 64 MB of RAM when idle (the BitTorrent engine only starts with the first torrent), no database server and no Node.js at runtime.

> [!NOTE]
> Status: usable, version 0.x. What comes next is in the [roadmap](#-roadmap).

## 📋 Requirements

FetchBox is small, and a download manager is mostly waiting for the network and the disk, so almost any always-on machine will do: a mini PC, a NAS with Docker, a Raspberry Pi, a VM or a Proxmox container. What it asks for:

| | Minimum | Recommended |
| --- | --- | --- |
| 🧠 **CPU** | 1 core, 64-bit: x86_64 or ARM64 (Raspberry Pi 3 and newer with a 64-bit OS) | 2 cores; 4 cores for a gigabit line with many files at once, or for torrents that move tens of MB/s (every piece is hashed) |
| 🧮 **RAM** | 256 MB free (the program itself needs about 64 MB when idle; a running torrent adds a few tens of MB) | 512 MB to 1 GB, more when the list holds many thousands of entries or you run many torrents with many peers each |
| 🐧 **System** | a 64-bit Linux with glibc 2.36 or newer and systemd (or Docker) | **Debian 12 or 13** (what the release is built and tested on), or Ubuntu 22.04 / 24.04 LTS, Raspberry Pi OS 64-bit, Fedora |
| 🗄️ **Disk for the program** | 100 MB (the binary, and a database that stays small even after thousands of downloads) | the system disk; on a Raspberry Pi an SSD or a good SD card |
| 📥 **Disk for downloads** | free space for the files that are in progress at the same time (the finished ones stay) | **a separate disk, partition or network share**. An SSD is the recommended choice, but a CMR hard disk is entirely enough, especially for lines up to 1 Gbit/s. ⚠️ Not a memory card, and not an SMR disk if your line is fast (see [below](#-disk-speed-and-memory-cards)) |
| 📡 **Network** | outgoing access to the sources you download from | for torrents: port **6881, TCP and UDP**, reachable from the internet (forwarded on the router, open in the firewall), IPv4; and upload bandwidth you are willing to give, as a torrent keeps sending until you stop it |
| 🌐 **Browser** | any current Chrome, Edge, Firefox or Safari | the same, over HTTPS (installing the web app and the paste button need a secure context) |

A few notes behind those numbers:

- **A separate disk for downloads** is the one recommendation that matters most. A full download disk then cannot take the system or the database down with it, FetchBox shows exactly the space that is meant for downloads, you can size, replace or back it up on its own, and on a Raspberry Pi it keeps the heavy writing off the SD card. Mount it, make the folder writable for the service's user, and point `FETCHBOX_DOWNLOAD_DIR` or `--download-dir` at it (in a Proxmox container map a host folder, see [Proxmox LXC](#proxmox-lxc)).
- **File system**: ext4, XFS or Btrfs are the best fit. A file is created at its full size before the first byte arrives, which costs nothing where files can be sparse (ext4, XFS, Btrfs, APFS); on exFAT, FAT or some network shares the whole size may be allocated up front, and FAT32 cannot hold files over 4 GB at all.
- **CPU**: the work is network and disk. One core is enough for a few downloads at 100 Mbit/s; a gigabit line with HTTPS and many connections per file is where a second and third core help. The limits you set in Settings keep it below whatever the machine can do.
- **Checksum verification** is the one step that is not just waiting: when a download has a checksum, the finished file is read back once from the disk and hashed (MD5, SHA-1, SHA-256 or SHA-512). That costs CPU time and a sequential read of the whole file, so a large file on a slow disk or a small ARM board takes a while (the SHA instructions of the CPU are used where present). It does not hold a download slot, so the next download starts meanwhile, but it competes with the running ones for the disk. Leave the checksum off for files you do not need to verify.
- **Torrents** ask a little more than downloads, mostly of the disk and the network:
  - **Disk.** The pieces of a torrent come from many peers in no particular order, so the writes jump around the file (a download writes it from start to end), and seeding reads pieces at random. An SSD is comfortable; a CMR hard disk copes with a few torrents; an SMR disk or a memory card does not (see [below](#-disk-speed-and-memory-cards)). Adding a torrent whose files are already on the disk reads and checks all of them once; a restart does not repeat that for torrents that were complete or are resumed (the engine saves which pieces are done).
  - **Open files.** The engine keeps every file of every torrent open, and every peer is a connection, so a torrent of thousands of files needs thousands of file descriptors. FetchBox raises its limit to the hard limit of the system at start (no root needed); a system or container with a low hard limit (some NAS firmware, `ulimit -Hn`) needs it raised, or such a torrent will not start.
  - **CPU and memory.** Measured with one 256 MiB torrent on the loopback (release build, x86_64): it was received, hashed and written in about 2 seconds using about 1.5 seconds of CPU time, and the whole program peaked at 20 MB of memory (it is 11 MB with no torrent). Not measured: a large swarm or dozens of torrents; both grow with the number of torrents and connected peers.
  - **Network.** Torrents work without an open port, but only the peers you connect to can be used, so speeds and the number of peers are lower. IPv6 is not used yet. The speed limits for torrents are in **Settings** (they are separate from those of downloads).
- **Operating systems** older than Debian 12 or Ubuntu 22.04, 32-bit systems (including 32-bit Raspberry Pi OS) and musl systems such as Alpine are not supported by the release binaries.

### 💽 Disk speed and memory cards

> [!WARNING]
> **Downloading to a memory card (SD, microSD) is NOT recommended, above all when the system is on that card.** A download manager writes all day, a card has little endurance and slow writes, and when it wears out, fills up or is cut off in the middle of a write, it takes the operating system with it. On a Raspberry Pi keep the system on the card if you must, but put the downloads on an SSD or a hard disk on USB. The same goes for USB sticks.

**What happens when the line is faster than the disk.** FetchBox reads from the network only as fast as it can hand the data to the disk, so nothing piles up in memory (a few MB per connection) and the download does not fail: the server is simply slowed down through the connection. The data first goes into the operating system's write cache, so the first seconds, or the first gigabytes on a machine with a lot of free RAM, can run at line speed and look faster than the disk really is; once the cache is full the speed falls to what the disk sustains. The speed shown in the app is the speed at which data is accepted for writing.

- **SMR hard disks** (shingled, common in cheap 2.5" and some "desktop" drives) take short bursts quickly into a small cache area and then rewrite whole bands of tracks in the background. A long download of many gigabytes can drop to a fraction of the advertised speed, and writes to several places of the file at once make it worse. If your line is faster than about 100 MB/s, or you download huge files, prefer an SSD or a CMR drive (the data sheet says which; "SMR" is often not stated, the model number tells).
- **An SSD is the recommended choice, but a CMR hard disk is entirely enough** (especially for lines up to 1 Gbit/s), and it is rarely the bottleneck; a RAID array even less. A conventional (CMR) 3.5" disk sustains roughly 100 to 250 MB/s of sequential writes, which covers a 1 Gbit/s line (about 120 MB/s); a 2.5" one is nearer the lower end and may be the limit at 1 Gbit/s. Striped arrays (RAID 0, 5, 6, 10, ZFS or Btrfs over several disks) add the speed of their disks together, so even a 2.5 or 10 Gbit/s line is within reach, while RAID 1 writes at the speed of one disk. Downloads are large sequential writes, which arrays handle well (torrents are scattered ones, which they handle less well). The one thing to check is that the drives really are CMR: the name on the box does not always tell, and not every maker states it in the product name, so look up the exact model number in the maker's CMR/SMR list or its data sheet before you rely on it. A RAID is not a backup, and arrays built from SMR drives are known for painfully slow rebuilds.
- **Torrents are the harder case.** Pieces arrive from many peers in random order, so even one torrent is many scattered writes, and a torrent that is being sent is read at random places. This is where an SSD pays off and an SMR disk or a memory card gives up first. Fewer torrents at a time, and a limit on the receiving speed in Settings, are the gentle options on a hard disk.
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

Open <http://localhost:8080> and create your account with the setup code ([details](#-signing-in)). The database goes to `./data` and the downloads to `./downloads`; change the `volumes:` in `docker-compose.yml` to put the downloads elsewhere (for example `/mnt/nas/downloads:/downloads`). Inside the container FetchBox listens on port **80**, and the compose file publishes it as **8080** on the host. For another port on the host put `FETCHBOX_HTTP_PORT=9000` in a `.env` file next to `docker-compose.yml`; `FETCHBOX_PORT` (default `80`) changes the port inside, and the mapping follows it. Running the image without compose, map the container's port 80: `docker run -p 8080:80 ...` (older versions of the image listened on 8080 inside). The image is Debian 13 based and runs as the unprivileged user 65532 without a shell, so give it the two folders before the first start (otherwise Docker creates them for root and FetchBox cannot write to them):

```bash
mkdir -p data downloads && sudo chown 65532:65532 data downloads
```

### Proxmox LXC

On the Proxmox host, in the style of the [community-scripts](https://github.com/community-scripts/ProxmoxVE):

```bash
export COMMUNITY_SCRIPTS_URL=https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/deploy/proxmox
bash -c "$(curl -fsSL $COMMUNITY_SCRIPTS_URL/ct/fetchbox.sh)"
```

It creates a small Debian 13 container (the web interface is on port **80**, so the address is just `http://<ip>`), installs the `.deb` package from the latest [release](https://github.com/fwitkowski17/WebDownloadManager/releases) and can bind-mount a directory of the host as `/downloads`. See [deploy/proxmox/README.md](deploy/proxmox/README.md) for the options, the permissions of the mapped folder and updating.

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

(Or download the file from the Releases page and give it to `apt install` / `dnf install`. Each file has a `.sha256` next to it.) The installation prints the port (**8080**; change it with `FETCHBOX_PORT` in `/etc/fetchbox/fetchbox.env`, then `systemctl restart fetchbox`: the service may listen on a port below 1024, such as 80, without being root) and the one-time **setup code** for creating the account (later: `journalctl -u fetchbox | grep 'setup code'`).

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

You need the Rust toolchain (version pinned by `rust-toolchain.toml`), Node.js 22 and a C compiler (`gcc` or `clang`, which the BitTorrent engine's cryptography needs; `build-essential` on Debian and Ubuntu):

```bash
./scripts/build.sh     # builds the web app, then target/release/fetchbox
```

## ⚙️ Configuration

Everything day to day (connections, limits, category folders, theme, language, update check) is in **Settings** in the web app and applies at once. The rest is on the command line and in the environment:

| Option | Meaning |
| --- | --- |
| `--data-dir <dir>` / `FETCHBOX_DATA_DIR` | where the database lives (default `./data`) |
| `--download-dir <dir>` / `FETCHBOX_DOWNLOAD_DIR` | where files are saved (default `./downloads`) |
| `--port <port>` / `FETCHBOX_PORT` | TCP port of the web interface (default `8080`; the Docker image listens on `80` inside, published as `8080`, and a new Proxmox container uses `80`). For the packages and the host script put it in `/etc/fetchbox/fetchbox.env`; a port below 1024 works there too |
| `--password-file <file>` | read the password from a file, e.g. a Docker secret |
| `--no-auth` | no password at all: see the caution below |
| `--block-private-urls` / `FETCHBOX_BLOCK_PRIVATE_URLS` | refuse links to loopback, private and link-local addresses (SSRF protection); off by default so you can fetch from a NAS |
| `--torrent-port <port>` / `FETCHBOX_TORRENT_PORT` | TCP and uTP port that BitTorrent peers connect to (default `6881`); nothing listens on it until the first torrent is added |
| `FETCHBOX_PASSWORD` | a fixed password (8+ characters) instead of one created in the browser |
| `FETCHBOX_INSTALL_METHOD` | how FetchBox was installed: `docker`, `package`, `lxc`, `script` or `source`. The installers set it, and the update notice shows only the steps of that method (without it, every way is listed) |
| `FETCHBOX_UPDATE_URL` | use another releases endpoint for the update check (a mirror, a fork) |
| `--log-activity` / `FETCHBOX_LOG_ACTIVITY` | log every added, paused, finished and removed download (with its average speed), settings changes, the download hours and sign-ins; without it the console shows start-up information, warnings and errors |
| `RUST_LOG` | log level of everything else, e.g. `debug` (default `info`) |

> [!CAUTION]
> `--no-auth` lets **anyone who can reach the port** add and delete downloads and write files into your download folder. Use it only on a network you fully trust, never on one that is reachable from the internet.

> [!IMPORTANT]
> Put a reverse proxy with HTTPS in front when FetchBox is reachable beyond your LAN, and send `X-Forwarded-Proto: https` so the session cookie gets `Secure`.

The live updates use server-sent events: switch response buffering off for `/api/events` (the server already sends `X-Accel-Buffering: no`).

## 🗄️ Several disks

Mount each disk as a folder **inside** the download folder, for example `/downloads/hdd10tb` and `/downloads/ssd1`. FetchBox treats every folder there that is a file system of its own (a mount point) or a symbolic link to a folder (`ln -s /mnt/hdd /downloads/hdd`; in a container the target must be mounted too) as a *disk*:

```yaml
    volumes:
      - ./data:/data
      - /mnt/hdd10tb:/downloads/hdd10tb
      - /mnt/ssd1:/downloads/ssd1
```

- **First run.** When more than the download folder itself is found, the setup asks which disk new downloads go to (after the password, since the list is behind the login). Nothing is chosen for you: the download folder may be the small system disk of a container.
- **Settings > Download disk** shows each disk with its path and capacity and lets you change the default, for downloads and, if you like, separately for torrents. Changing it **moves nothing**: every download keeps the disk it was added to, with its entry in the list and its file.
- **Adding a download or a torrent** has a *Save to* choice (the default disk is preselected), with the free space of the disk you pick. The space checks, name conflicts and the disk ring follow it.
- **Warnings.** In Docker, FetchBox tells when the download folder is inside the container (nothing from the host is mounted there, so what is saved is lost when the container is recreated), when a disk is on the same file system as the container's data on the host (usually the host's system disk: the path on the host may not have been a mounted disk when the container started), and when an empty folder in the download folder is not a disk (a mount that did not happen looks like that). They are hints, from the first-run step and from Settings; nothing is refused because of them.
- **A disk that is not mounted** is refused (an empty folder on the system disk would be filled in its place): adding to it is an error, and a download that was on it fails with a message until the disk is back, then it can be resumed. If the default disk is gone, adds that rely on it are refused until you pick another disk or the download folder.
- The folders `torrents`, `photo`, `music`, `video`, `program`, `archive` and `other` are never offered as disks (they are the torrent folder and the category folders, which may sit on a disk of their own as before). **Torrents** are saved in `torrents/` on their disk (inside the download folder for the download folder itself, `<disk>/torrents/` on a disk; a torrent of several files gets a folder of its name, and a magnet link always one).

## 🌍 Sources and logins

Besides `http://` and `https://`, FetchBox downloads from FTP and SFTP servers: paste the address in the add dialog like any other link. A folder shows a list to choose files from. Pause, resume from the saved offset, limits, hours, the queue and the checksum check work as for HTTP.

| Address | Protocol | Notes |
| --- | --- | --- |
| `ftp://host/path/file` | FTP, plain | Port 21. Anonymous unless you give a login. The password is not encrypted on the way. |
| `ftps://host/path/file` | FTP over TLS | Port 21 with `AUTH TLS`, or TLS from the first byte on port 990. The certificate must be one that browsers trust. |
| `sftp://host/path/file` | SSH file transfer | Port 22. Needs a user name and a password. |

- **Paths.** FTP paths start in the folder the login lands in, so `ftp://host/pub/a.iso` is `pub/a.iso` there; write `ftp://host//srv/a.iso` for the root. SFTP paths are absolute (`sftp://host/srv/a.iso`); write `sftp://host/~/a.iso` for a path in the home folder.
- **Logins.** Tick *This address needs a login* (for SFTP it is always on), or write `ftp://user:password@host/file` and FetchBox takes the login out of the address. The password is kept on the server, in its database, so a paused or restarted download can go on; it is never shown again, returned by the API or written to the log. Plain `http://` and `ftp://` send it unencrypted, and the dialog says so. The same login also works for HTTP servers that ask for basic authentication.
- **SSH servers must be trusted first.** SFTP has no certificate authorities: a server is believed because its key is the one you accepted. The first time you use a server, the dialog shows its fingerprint (`SHA256:...`, the same as `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub` prints on the server); compare it and press *Trust this server*. No password is sent before that. If the key of a trusted server changes, nothing is sent and the dialog says so, which is what a reinstalled server looks like but also what a man in the middle looks like. The accepted keys are listed under **Settings → Trusted SSH servers**, where you can forget them.
- **One connection.** FTP and SFTP use a single connection per download, whatever *Connections per download* says: servers often allow only a few per user, and these protocols do not need more to fill a line.

> [!NOTE]
> Not supported yet: SFTP with a private key (only a password), FTPS servers with a self-signed certificate, active-mode FTP, and changing the login of a download after it was added (remove it and add it again). FTP and SFTP are tested against vsftpd and OpenSSH and against test servers, but not against every server out there: if one misbehaves, please open an [issue](https://github.com/fwitkowski17/WebDownloadManager/issues) with its name.

## 🧲 Torrents

Open **Torrents** in the side menu and add a magnet link or a `.torrent` file (chosen, or dropped on the dialog; one magnet link per line adds several). Torrents are a list of their own, apart from the downloads: they go to `torrents/` inside the download folder, and the queue, the download hours and the download speed limits do not apply to them (torrents have hours and limits of their own, below). **Downloads** and **Torrents** are chosen at the top of the side menu. A click on a torrent opens a panel at the bottom with its overview, **peers** (address, client, link, speed of each direction, bytes), **files** and a **speed chart**.

| Status | Meaning |
| --- | --- |
| Fetching metadata | a magnet link: the names of the files are being fetched from other peers |
| Checking files | files already on the disk are being checked |
| Connecting to peers | running, no peer is connected yet |
| Downloading / Sending | receiving data / complete and sending to others (it goes on until you stop the torrent) |
| Paused / Finished | stopped by you / complete and stopped |

Speed limits for all torrents together (receiving and sending) and the **torrent hours** are in **Settings**. Outside the torrent hours torrents are stopped (nothing is received or sent) and go on by themselves when the hours open; a torrent you stopped yourself stays stopped, and **Start now** lets one torrent run regardless. A completed torrent keeps sending, so a limit on sending keeps your upload line free; stop a torrent when you are done with it.

- **Open the port** for the best speeds: forward `6881` (TCP and UDP) on your router and allow it in the firewall; with Docker the compose file already publishes it. Without it torrents still work, only fewer peers can reach you.
- **Seeds are not counted separately.** The BitTorrent library behind FetchBox does not tell which peers have the whole file, so the numbers are peers: connected, connecting, known.
- Not there yet: choosing which files of a torrent to download, a queue, stopping at a ratio, IPv6 and UPnP.
- **`--block-private-urls` does not cover torrents.** It refuses trackers at private addresses, but peers come from the swarm and are not filtered. Do not rely on it to keep BitTorrent traffic off your LAN.
- Only download what you have the right to download. FetchBox is a tool; what you fetch and share is up to you.

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

- 🧲 **More for torrents**: choosing the files of a torrent, a queue, stopping at a ratio, torrents in the statistics, a count of seeds, IPv6 and UPnP (what is there now is [above](#-torrents)).
- 🌍 **More kinds of sources**, behind the same queue, limits, hours, statistics and checksum check (FTP, FTPS and SFTP are done, see [above](#-sources-and-logins)):
  - **WebDAV** (for example a Nextcloud or a NAS), over HTTPS with the same segmented download where the server allows ranges.
  - **S3-compatible storage** (Amazon S3, Cloudflare R2, MinIO, Backblaze B2 and the like): a pre-signed link should already work as an ordinary HTTPS address; a direct `s3://bucket/key` address with your own keys is the part to be added. Providers differ in the details, so each one will be checked before it is listed as supported.
  - The rest of FTP and SFTP: SFTP with a private key, FTPS with a self-signed certificate (trusted by its fingerprint, like an SSH key).
- 🔑 **More ways to authorize a download**: a user name and password already work for HTTP (basic authentication), FTP, FTPS and SFTP. Still to come: tokens and your own headers (a private server, a Nextcloud share, a link behind a login, a private S3 bucket), and changing the login of a download that already exists.
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
