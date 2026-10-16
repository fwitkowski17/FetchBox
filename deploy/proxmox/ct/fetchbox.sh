#!/usr/bin/env bash
_cs_boot="${COMMUNITY_SCRIPTS_CORE_DIR:-$(dirname "${BASH_SOURCE[0]}")/../../core}/core/build.func"
source "$_cs_boot" 2>/dev/null || source <(curl -fsSL "${COMMUNITY_SCRIPTS_CORE_URL:-https://raw.githubusercontent.com/community-scripts/core/main}/core/build.func")
# Copyright (c) 2026 fwitkowski17
# Author: fwitkowski17
# License: MIT
# Source: https://github.com/fwitkowski17/WebDownloadManager

APP="FetchBox"
var_tags="${var_tags:-downloads;manager}"
# A small program: one binary, about 64 MB of RAM while running. Downloads go to /downloads, which
# lives on the container's disk unless a host directory is mapped there (see var_downloads_dir).
var_cpu="${var_cpu:-1}"
var_ram="${var_ram:-512}"
var_disk="${var_disk:-8}"
var_os="${var_os:-debian}"
var_version="${var_version:-13}"
var_arm64="${var_arm64:-yes}"
var_unprivileged="${var_unprivileged:-1}"

# Optional: a directory on the Proxmox host that becomes /downloads in the container, so files land
# on a big disk or a NAS share instead of the container's own disk. Asked for in the interactive
# run; set it to skip the question (or to map non-interactively):
#   var_downloads_dir=/mnt/pve/nas/downloads bash -c "$(curl ...)"
var_downloads_dir="${var_downloads_dir:-}"

header_info "$APP"
variables
color
catch_errors

FB_REPO="fwitkowski17/WebDownloadManager"
FB_LEGACY_DIR="/opt/fetchbox" # where the first versions of this installer put the program
FB_UID=1000 # the user the service runs as, inside the container (the install script creates it)
FB_DOWNLOADS="/downloads"

# The CPU in the name of the package on the GitHub release (built by .github/workflows/release.yml).
function fetchbox_deb_arch() {
  arch_resolve amd64 arm64
}

# The latest release: its version (without the "v") and the address of the .deb for this CPU.
# Sets FB_LATEST and FB_DEB_URL; returns 1 when there is no such release.
function fetchbox_latest() {
  local json
  json="$(curl -fsSL -m 30 "https://api.github.com/repos/${FB_REPO}/releases/latest" 2>/dev/null)" || return 1
  FB_LATEST="$(grep -o '"tag_name": *"[^"]*"' <<<"$json" | head -n1 | sed 's/.*: *"v\{0,1\}\(.*\)"/\1/')"
  FB_DEB_URL="$(grep -o "\"browser_download_url\": *\"[^\"]*_$(fetchbox_deb_arch)\.deb\"" <<<"$json" | head -n1 | sed 's/.*: *"\(.*\)"/\1/')"
  [[ -n "$FB_LATEST" && -n "$FB_DEB_URL" ]]
}

# Download FB_DEB_URL, check it against its .sha256 and install it (an upgrade when FetchBox is
# there). The settings in /etc/fetchbox/fetchbox.env are kept; the package restarts a running service.
function fetchbox_install_deb() {
  local dir file
  dir="$(mktemp -d)"
  file="${dir}/$(basename "$FB_DEB_URL")"
  curl -fsSL -o "$file" "$FB_DEB_URL"
  curl -fsSL -o "${file}.sha256" "${FB_DEB_URL}.sha256"
  (cd "$dir" && sha256sum --check --status "$(basename "$file").sha256")
  $STD apt-get install -y -o Dpkg::Options::=--force-confold "$file"
  rm -rf "$dir"
}

# The first versions of this installer put the program in /opt/fetchbox with its own unit. Move it
# to the package: data and settings are carried over, the downloads (/downloads) are not touched.
function fetchbox_migrate_legacy() {
  systemctl stop fetchbox
  install -d -m 0750 -o fetchbox -g fetchbox /var/lib/fetchbox
  if [[ -d "${FB_LEGACY_DIR}/data" ]]; then
    cp -a "${FB_LEGACY_DIR}/data/." /var/lib/fetchbox/
    chown -R fetchbox:fetchbox /var/lib/fetchbox
  fi
  install -d /etc/fetchbox
  [[ -f "${FB_LEGACY_DIR}/fetchbox.env" ]] && cp -a "${FB_LEGACY_DIR}/fetchbox.env" /etc/fetchbox/fetchbox.env
  install -d /etc/systemd/system/fetchbox.service.d
  printf '[Service]\nEnvironment=FETCHBOX_DOWNLOAD_DIR=%s\n' "$FB_DOWNLOADS" >/etc/systemd/system/fetchbox.service.d/10-downloads.conf
  rm -f /etc/systemd/system/fetchbox.service
  systemctl daemon-reload
  fetchbox_install_deb
  # The old copy goes last: until the package is installed it is the way back.
  rm -rf "$FB_LEGACY_DIR"
}

# ------------------------------------------------------------------------------
# Updating (runs inside the container: `update`)
# ------------------------------------------------------------------------------

function update_script() {
  header_info
  check_container_storage
  check_container_resources
  local installed=""
  installed="$(dpkg-query -W -f='${Version}' fetchbox 2>/dev/null || true)"
  if [[ -z "$installed" && ! -x "${FB_LEGACY_DIR}/fetchbox" ]]; then
    msg_error "No ${APP} Installation Found!"
    exit
  fi
  if ! fetchbox_latest; then
    msg_error "Could not find the latest ${APP} release (with a .deb for $(fetchbox_deb_arch)): https://github.com/${FB_REPO}/releases"
    exit
  fi
  if [[ -n "$installed" && "$installed" == "$FB_LATEST" ]]; then
    msg_ok "No update required. ${APP} is already at ${FB_LATEST}"
    exit
  fi

  if [[ -z "$installed" ]]; then
    msg_info "Moving ${APP} from ${FB_LEGACY_DIR} to the package (${FB_LATEST})"
    fetchbox_migrate_legacy
    msg_ok "Moved ${APP} to the package"
  else
    # Only the program is replaced: the database (/var/lib/fetchbox), the settings
    # (/etc/fetchbox/fetchbox.env) and the downloads are not touched. The package's own script
    # restarts the service.
    msg_info "Updating ${APP} ${installed} -> ${FB_LATEST}"
    fetchbox_install_deb
    msg_ok "Updated ${APP}"
  fi
  msg_ok "Updated successfully!"
  exit
}

# Before a container is made: is there a release with a package for this CPU? (The installer takes
# the program from there; without it the container would be created and then fail.)
function check_release() {
  if ! fetchbox_latest; then
    msg_error "No ${APP} release with a .deb for $(fetchbox_deb_arch) has been published yet: https://github.com/${FB_REPO}/releases"
    exit 1
  fi
}

# ------------------------------------------------------------------------------
# Downloads folder: bind-mount a directory of the Proxmox host as /downloads
# ------------------------------------------------------------------------------

# Where the service's user (uid/gid FB_UID in the container) lives on the host. A privileged
# container uses the same numbers; an unprivileged one adds the start of root's subordinate range
# (100000 unless the host says otherwise).
function fetchbox_host_id() {
  local file="$1" base=0
  if pct config "$CTID" 2>/dev/null | grep -q '^unprivileged: 1'; then
    base="$(awk -F: '$1=="root"{print $2; exit}' "$file" 2>/dev/null)"
    base="${base:-100000}"
  fi
  echo $((base + FB_UID))
}

function map_downloads() {
  command -v pct >/dev/null 2>&1 || return 0
  [[ -n "${CTID:-}" ]] || return 0

  local dir="$var_downloads_dir"
  if [[ -z "$dir" && "${PHS_SILENT:-0}" != "1" && -t 0 ]] && command -v whiptail >/dev/null 2>&1; then
    dir="$(whiptail --backtitle "Proxmox VE Helper Scripts" --title "DOWNLOADS FOLDER" --inputbox \
      "Directory on this Proxmox host to use for downloads (it appears as ${FB_DOWNLOADS} in the container).\n\nLeave empty to keep downloads on the container's own disk." \
      13 72 "" 3>&1 1>&2 2>&3)" || dir=""
  fi
  [[ -n "$dir" ]] || return 0

  # The value goes into a `pct set` option string: a comma would add options of its own.
  if [[ "$dir" != /* || "$dir" == *,* || ! -d "$dir" ]]; then
    msg_warn "Not mapping '${dir}': it must be an existing absolute directory on the host (without commas)"
    return 0
  fi

  local uid gid
  uid="$(fetchbox_host_id /etc/subuid)"
  gid="$(fetchbox_host_id /etc/subgid)"
  if [[ "$(stat -c '%u:%g' "$dir")" != "${uid}:${gid}" ]]; then
    msg_info "Giving ${dir} to the container's ${APP} user (${uid}:${gid})"
    if chown "${uid}:${gid}" "$dir" 2>/dev/null; then
      msg_ok "Gave ${dir} to the container's ${APP} user"
    else
      msg_warn "Could not change the owner of ${dir} (a network share?). ${APP} must be able to write there: run 'chown ${uid}:${gid} ${dir}' on the share's server, or see the README"
    fi
  fi

  msg_info "Mapping ${dir} to ${FB_DOWNLOADS}"
  if pct set "$CTID" -mp0 "${dir},mp=${FB_DOWNLOADS},backup=0" >/dev/null 2>&1 && pct reboot "$CTID" >/dev/null 2>&1; then
    msg_ok "Mapped ${dir} to ${FB_DOWNLOADS}"
  else
    msg_warn "Could not map ${dir}; add it yourself: pct set ${CTID} -mp0 ${dir},mp=${FB_DOWNLOADS},backup=0 && pct reboot ${CTID}"
  fi
}

# The setup code is made anew every time the service starts, and the container is restarted while it
# is set up (map_downloads), so a code seen earlier may be gone. Ask the service that is running now,
# and read only the log of its current run (a restart in between must not give an older code back).
# Waits for the container to be up again; prints nothing when it cannot find one.
function fetchbox_setup_code() {
  command -v pct >/dev/null 2>&1 && [[ -n "${CTID:-}" ]] || return 0
  local code="" _
  for _ in $(seq 1 45); do
    # shellcheck disable=SC2016 # the variables are meant to expand inside the container, not here
    code="$(pct exec "$CTID" -- sh -c '
      systemctl is-active --quiet fetchbox || exit 0
      id="$(systemctl show -p InvocationID --value fetchbox)"
      [ -n "$id" ] || exit 0
      journalctl "_SYSTEMD_INVOCATION_ID=$id" --no-pager 2>/dev/null' 2>/dev/null |
      grep -o 'setup code [A-Z0-9]\{8\}' | tail -n1 | awk '{print $3}' || true)"
    [[ -n "$code" ]] && break
    sleep 1
  done
  echo "$code"
}

start
check_release
build_container
map_downloads
description
setup_code="$(fetchbox_setup_code)"

msg_ok "Completed successfully!\n"
echo -e "${CREATING}${GN}${APP} setup has been successfully initialized!${CL}"
if [[ -n "$setup_code" ]]; then
  echo -e "${INFO}${YW}Open it using the following URL and enter the setup code ${BGN}${setup_code}${CL}"
else
  echo -e "${INFO}${YW}Open it using the following URL and enter the setup code, which you get in the container with:${CL}"
  echo -e "${INFO}${YW}journalctl -u fetchbox | grep 'setup code' | tail -n 1${CL}"
fi
echo -e "${GATEWAY}${BGN}http://${IP}:8080${CL}"
