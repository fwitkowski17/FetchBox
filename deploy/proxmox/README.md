# FetchBox in a Proxmox LXC container

An installer in the style of the [community-scripts](https://github.com/community-scripts/ProxmoxVE) ("Proxmox VE Helper Scripts"): it runs on the Proxmox host, creates a Debian container, installs the prebuilt FetchBox binary from the GitHub release and starts it as a systemd service. It uses the community-scripts engine for the container dialogs, so you get their usual menus (default or advanced settings, storage, network, ...).

```
deploy/proxmox/
├── ct/fetchbox.sh                 # runs on the host: container settings, update, downloads folder
├── install/fetchbox-install.sh    # runs in the container: user, binary, service
└── json/fetchbox.json             # metadata for the script catalogue
```

## Install

Run on the Proxmox host (as root). The engine is fetched from community-scripts; `COMMUNITY_SCRIPTS_URL` makes it take **this** repository's `ct/` and `install/` scripts instead of theirs:

```bash
export COMMUNITY_SCRIPTS_URL=https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/deploy/proxmox
bash -c "$(curl -fsSL $COMMUNITY_SCRIPTS_URL/ct/fetchbox.sh)"
```

(To try a branch, put its name instead of `main`.) At the end the script prints the address (`http://<ip>:8080`) and the **setup code** you need to create the account in the browser. Forgot it? In the container: `journalctl -u fetchbox | grep 'setup code'`. To use a fixed password instead, put `FETCHBOX_PASSWORD=...` into `/opt/fetchbox/fetchbox.env` and `systemctl restart fetchbox`.

The defaults are 1 CPU core, 512 MB RAM and 8 GB disk, Debian 13, unprivileged (FetchBox itself needs about 64 MB). Override them like in the other scripts (`var_cpu=2 var_ram=1024 bash -c ...`) or in the advanced dialog.

**It needs a published release.** The installer takes the program from the latest GitHub Release of the repository (the archive `fetchbox-linux-x86_64.tar.gz` or `-aarch64`); nothing is compiled in the container. Before it creates the container the script checks that such a release exists and stops with a message if not. See [Releasing](#releasing).

### Why Debian, not Alpine

FetchBox is a normal glibc program (the release binaries are built on Debian 12, so that they also run on Debian 12 and 13, Ubuntu 22.04 and newer and Raspberry Pi OS; the container itself is Debian 13), systemd keeps it running, and the framework's defaults are Debian. Alpine uses musl and OpenRC: it would need a separate static (musl) build in the release and another service definition, to save about 100 MB; nothing here is tested on it. If you want it, it is a separate piece of work.

### What ends up in the container

One binary, `/opt/fetchbox/fetchbox` (about 10 MB; the web interface is inside it), run by systemd as the unprivileged user `fetchbox` (uid 1000). The database is in `/opt/fetchbox/data`, the service settings in `/opt/fetchbox/fetchbox.env`, the downloads go to `/downloads`. No compiler, no Node.js.

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

## Releasing

The release is made by the workflow `.github/workflows/release.yml`; it needs nothing but a tag:

1. Put the new version in `Cargo.toml` (`[workspace.package] version`) and `web/package.json`, merge it.
2. `git tag v0.6.1 && git push origin v0.6.1` (the tag is `v` + the version; the workflow fails when they differ, because the app compares the tag with its own version to announce updates).
3. The workflow builds the web interface, then the binary for **x86_64** and **aarch64** Linux (in a Debian 12 container, which sets the oldest system they run on), and publishes a GitHub Release with `fetchbox-linux-x86_64.tar.gz`, `fetchbox-linux-aarch64.tar.gz` and a `.sha256` file for each, with release notes generated from the merged pull requests. A tag with a dash (`v0.7.0-rc.1`) makes a pre-release, which neither the installer nor the app's update check picks up.

You may also create the release by hand on GitHub first (with the tag): the workflow then only attaches the files.

The release notes are what the update dialog in the app shows, so they are worth a look before you publish more.

## Update and everyday use

In the container (the console of the CT) run `update`: it looks for a newer release, stops the service, replaces the binary and starts it again. The database, the settings and the downloads stay. If `update` cannot find the script (it uses `COMMUNITY_SCRIPTS_URL` as it was when the container was made), run it by hand: `bash -c "$(curl -fsSL https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/deploy/proxmox/ct/fetchbox.sh)"` inside the container.

| What | Command (in the container) |
| --- | --- |
| Status / logs | `systemctl status fetchbox`, `journalctl -u fetchbox -f` |
| Restart | `systemctl restart fetchbox` |
| Settings of the service | `/opt/fetchbox/fetchbox.env` |
| Forgot the password | `systemctl stop fetchbox && /opt/fetchbox/fetchbox --data-dir /opt/fetchbox/data reset-password` as user `fetchbox` (`runuser -u fetchbox -- ...`), then start it and use the new setup code |

Put a reverse proxy with HTTPS in front of it when it is reachable beyond your LAN.

## Status

Written to follow the structure of the community-scripts scripts, but **not tried on a real Proxmox host yet**, and the release workflow has not run on GitHub yet either. What is checked: the scripts pass `shellcheck` and `bash -n`, the workflow passes `actionlint`, the helper logic (mapping the downloads folder, user ids, the release pre-check) is tested against stubs, the archive layout was tried, and the application itself is tested in the repository. Report or fix what a real run turns up. These scripts are not part of the community-scripts project.
