# FetchBox in a Proxmox LXC container

An installer in the style of the [community-scripts](https://github.com/community-scripts/ProxmoxVE) ("Proxmox VE Helper Scripts"): it runs on the Proxmox host, creates a Debian container, builds FetchBox in it and starts it as a systemd service. It uses the community-scripts engine for the container dialogs, so you get their usual menus (default or advanced settings, storage, network, ...).

```
deploy/proxmox/
├── ct/fetchbox.sh                 # runs on the host: container settings, update, downloads folder
├── install/fetchbox-install.sh    # runs in the container: build and service
└── json/fetchbox.json             # metadata for the script catalogue
```

## Install

Run on the Proxmox host (as root). The engine is fetched from community-scripts; `COMMUNITY_SCRIPTS_URL` makes it take **this** repository's `ct/` and `install/` scripts instead of theirs:

```bash
export COMMUNITY_SCRIPTS_URL=https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/deploy/proxmox
bash -c "$(curl -fsSL $COMMUNITY_SCRIPTS_URL/ct/fetchbox.sh)"
```

(To try a branch, put its name instead of `main`.) At the end the script prints the address (`http://<ip>:8080`) and the **setup code** you need to create the account in the browser. Forgot it? In the container: `journalctl -u fetchbox | grep 'setup code'`. To use a fixed password instead, put `FETCHBOX_PASSWORD=...` into `/opt/fetchbox/fetchbox.env` and `systemctl restart fetchbox`.

The defaults are 2 CPU cores, 2 GB RAM and 8 GB disk, Debian 13, unprivileged. Override them like in the other scripts (`var_cpu=4 var_ram=4096 bash -c ...`) or in the advanced dialog. The RAM is for compiling: the running program needs about 64 MB, so you may lower it after the installation, but an update compiles again.

### Why Debian, not Alpine

FetchBox is a normal glibc program (the Docker image is built on Debian too), systemd keeps it running, and the compiler and Node.js packages for building are in Debian's and the Rust/Node projects' own repositories. Alpine uses musl and OpenRC: it would need a separate static (musl) build and another service definition to save about 100 MB, and nothing in this installer is tested on it. If you want it, it is a separate piece of work.

### How it is installed

There are no prebuilt binaries yet, so the program is built from source inside the container: the newest GitHub release tag (or `main` while there is no release; `FETCHBOX_REF=<tag or branch>` pins one). It takes a few minutes, then the build tools (Rust, Node.js, the C compiler) and the sources are removed again, which leaves one binary of a few MB in `/opt/fetchbox`. The service runs as the unprivileged user `fetchbox` (uid 1000), the database is in `/opt/fetchbox/data` and downloads go to `/downloads`.

## Downloads folder on the host

Downloads are written to `/downloads` in the container, which by default lives on the container's disk. To use a big disk or a network share instead, map a directory of the **host** to it:

- the script asks for it in the interactive run ("DOWNLOADS FOLDER": an absolute path of an existing directory on the host; empty keeps the container's disk), or
- give it up front, which also works without any dialog: `var_downloads_dir=/mnt/pve/nas/downloads bash -c "..."`, or
- later, by hand: `pct set <CTID> -mp0 /mnt/pve/nas/downloads,mp=/downloads,backup=0 && pct reboot <CTID>`.

`backup=0` keeps the downloads out of container backups.

**Permissions.** The service must be allowed to write there. In an *unprivileged* container (the default) the container's user 1000 is user `100000 + 1000 = 101000` on the host (the start of root's range in `/etc/subuid`; the script reads it), so the script gives the directory to `101000:101000`. In a *privileged* container it is plain `1000:1000`. If the directory is a network share where `chown` is not allowed, the script says so and maps it anyway; then make it writable for that user yourself:

- **NFS:** on the share's server export it with `anonuid=101000,anongid=101000,all_squash`, or give the directory to that user there.
- **CIFS/SMB:** mount it on the host with `uid=101000,gid=101000` (and `file_mode=0664,dir_mode=0775`) in `/etc/fstab` or the Proxmox storage options, then map that mount point.
- Or make the directory writable for everyone (`chmod 777`) if you trust the host.

Only the directory you name is made available to the container.

## Update and everyday use

In the container (the console of the CT) run `update`; it asks GitHub for the newest release, and when there is a newer one (or a new commit of `main`) it stops the service, builds, installs and starts it again. The database and the downloads stay. If `update` cannot find the script (it uses `COMMUNITY_SCRIPTS_URL` as it was when the container was made), run it by hand: `bash -c "$(curl -fsSL https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/deploy/proxmox/ct/fetchbox.sh)"` inside the container.

| What | Command (in the container) |
| --- | --- |
| Status / logs | `systemctl status fetchbox`, `journalctl -u fetchbox -f` |
| Restart | `systemctl restart fetchbox` |
| Settings of the service | `/opt/fetchbox/fetchbox.env` |
| Forgot the password | `systemctl stop fetchbox && /opt/fetchbox/fetchbox --data-dir /opt/fetchbox/data reset-password` as user `fetchbox` (`runuser -u fetchbox -- ...`), then start it and use the new setup code |

Put a reverse proxy with HTTPS in front of it when it is reachable beyond your LAN.

## Status

Written to follow the structure of the community-scripts scripts, but **not tried on a real Proxmox host yet**: the helper logic (mapping the downloads folder, user ids, picking the release) is tested against stubs, the scripts pass `shellcheck` and `bash -n`, and the application itself is tested in the repository. Report or fix what a real run turns up. These scripts are not part of the community-scripts project.
