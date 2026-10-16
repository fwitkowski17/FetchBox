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
FB_DIR="/opt/fetchbox"
FB_UID=1000 # keep in step with ct/fetchbox.sh

msg_info "Creating the user and folders"
useradd --system --uid "$FB_UID" --user-group --home-dir "$FB_DIR" --no-create-home --shell /usr/sbin/nologin fetchbox
install -d -m 0755 "$FB_DIR"
install -d -o fetchbox -g fetchbox "${FB_DIR}/data" /downloads
msg_ok "Created the user and folders"

# The program is a single binary (the web interface is inside it), built by the release workflow of
# the repository. Nothing is compiled here.
fetch_and_deploy_gh_release "fetchbox" "${FB_REPO}" "prebuild" "latest" "${FB_DIR}" "fetchbox-linux-$(arch_resolve x86_64 aarch64).tar.gz"
chmod 0755 "${FB_DIR}/fetchbox"

msg_info "Creating the service"
cat <<'EOT' >"${FB_DIR}/fetchbox.env"
# Settings of the service (read by systemd). Restart after a change: systemctl restart fetchbox
#
# A fixed password (at least 8 characters). Leave it unset to create the account in the browser
# on first run, with the setup code from `journalctl -u fetchbox`. A password set here wins and
# cannot be changed in the web interface.
#FETCHBOX_PASSWORD=
EOT
chown root:fetchbox "${FB_DIR}/fetchbox.env"
chmod 0640 "${FB_DIR}/fetchbox.env"
cat <<EOT >/etc/systemd/system/fetchbox.service
[Unit]
Description=FetchBox download manager
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=fetchbox
Group=fetchbox
WorkingDirectory=${FB_DIR}
EnvironmentFile=-${FB_DIR}/fetchbox.env
ExecStart=${FB_DIR}/fetchbox --data-dir ${FB_DIR}/data --download-dir /downloads --port 8080
Restart=on-failure
RestartSec=3
# Nothing more is locked down on purpose: sandboxing options that need mount namespaces are
# refused in some container setups, and a service that does not start helps nobody.
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOT
systemctl enable -q --now fetchbox
msg_ok "Created the service"

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
