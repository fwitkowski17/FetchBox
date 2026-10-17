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
FB_PORT=80  # keep in step with ct/fetchbox.sh

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
# The web interface of this container is on port 80 (the package's default is 8080). The service may
# listen on it without being root: the package's unit grants that one capability. A drop-in is not a
# file of the package, and /etc/fetchbox/fetchbox.env (FETCHBOX_PORT=...) overrides it.
cat <<EOT >/etc/systemd/system/fetchbox.service.d/30-port.conf
[Service]
Environment=FETCHBOX_PORT=${FB_PORT}
EOT
# Tells the app it runs in this container, so its update notice says to run `update` here.
cat <<'EOT' >/etc/systemd/system/fetchbox.service.d/20-install-method.conf
[Service]
Environment=FETCHBOX_INSTALL_METHOD=lxc
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

# The first start prints a one-time setup code, but it is not shown here: the service makes a new code
# at every start, and this container may be restarted before it is set up (when a downloads folder is
# mapped), which would make a code shown now wrong. ct/fetchbox.sh asks the running service for the
# current one at the very end. All that is needed here is that the service has started.
for _ in $(seq 1 20); do
  systemctl is-active --quiet fetchbox && break
  sleep 1
done
if systemctl is-active --quiet fetchbox; then
  msg_ok "FetchBox is running"
else
  msg_warn "FetchBox is not running yet; see: systemctl status fetchbox"
fi

motd_ssh
customize
cleanup_lxc
