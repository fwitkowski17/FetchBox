#!/usr/bin/env bash

# Copyright (c) 2026 fwitkowski17
# Author: fwitkowski17
# License: MIT
# Source: https://github.com/fwitkowski17/WebDownloadManager

source /dev/stdin <<<"$FUNCTIONS_FILE_PATH"
color
verb_ip6
catch_errors
setting_up_container
network_check
update_os

FB_REPO="fwitkowski17/WebDownloadManager"
FB_UID=1000 # keep in step with ct/fetchbox.sh

# The program comes as a DEB from the latest GitHub release (built and tested by the release
# workflow). It brings the systemd unit, /etc/fetchbox/fetchbox.env and the data folder; nothing is
# compiled here.
msg_info "Creating the user and the downloads folder"
# Made here, with a fixed uid, so that a host folder mapped to /downloads can be given to it (see
# ct/fetchbox.sh). The package's own script keeps a user that already exists.
useradd --system --uid "$FB_UID" --user-group --home-dir /var/lib/fetchbox --no-create-home --shell /usr/sbin/nologin fetchbox
install -d -o fetchbox -g fetchbox /downloads
# The downloads go to /downloads instead of the package's default folder. A drop-in is not a file of
# the package, so an update never touches it, and it is in place before the first start.
install -d /etc/systemd/system/fetchbox.service.d
cat <<'EOT' >/etc/systemd/system/fetchbox.service.d/10-downloads.conf
[Service]
Environment=FETCHBOX_DOWNLOAD_DIR=/downloads
EOT
msg_ok "Created the user and the downloads folder"

msg_info "Installing FetchBox"
deb_arch="$(dpkg --print-architecture)"
deb_url="$(curl -fsSL -m 30 "https://api.github.com/repos/${FB_REPO}/releases/latest" |
  grep -o "\"browser_download_url\": *\"[^\"]*_${deb_arch}\.deb\"" | head -n1 | sed 's/.*: *"\(.*\)"/\1/')"
[[ -n "$deb_url" ]] || {
  msg_error "No ${deb_arch} .deb on the latest ${FB_REPO} release"
  exit 1
}
deb_dir="$(mktemp -d)"
deb_file="${deb_dir}/$(basename "$deb_url")"
curl -fsSL -o "$deb_file" "$deb_url"
curl -fsSL -o "${deb_file}.sha256" "${deb_url}.sha256"
(cd "$deb_dir" && sha256sum --check --status "$(basename "$deb_file").sha256")
# Installing starts the service (the package's script enables and starts it).
$STD apt-get install -y "$deb_file"
rm -rf "$deb_dir"
msg_ok "Installed FetchBox"

# The first start prints a one-time setup code; show it, since it is needed to create the account.
code=""
for _ in $(seq 1 20); do
  code="$(journalctl -u fetchbox --no-pager 2>/dev/null | grep -o 'setup code [A-Z0-9]\{8\}' | tail -n1 | awk '{print $3}' || true)"
  [[ -n "$code" ]] && break
  sleep 1
done
if [[ -n "$code" ]]; then
  msg_ok "Setup code (enter it in the web interface): ${code}"
else
  msg_warn "Could not read the setup code; see: journalctl -u fetchbox | grep 'setup code'"
fi

motd_ssh
customize
cleanup_lxc
