#!/usr/bin/env bash
_cs_boot="${COMMUNITY_SCRIPTS_CORE_DIR:-$(dirname "${BASH_SOURCE[0]}")/../../core}/core/build.func"
source "$_cs_boot" 2>/dev/null || source <(curl -fsSL "${COMMUNITY_SCRIPTS_CORE_URL:-https://raw.githubusercontent.com/community-scripts/core/main}/core/build.func")
# Copyright (c) 2026 fwitkowski17
# Author: fwitkowski17
# License: MIT | https://github.com/fwitkowski17/WebDownloadManager/raw/main/LICENSE
# Source: https://github.com/fwitkowski17/WebDownloadManager

APP="FetchBox"
var_tags="${var_tags:-downloads;manager}"
# Compiling the program needs memory and a few minutes; once installed it needs far less
# (about 64 MB). The RAM can be lowered afterwards, but an update compiles again.
var_cpu="${var_cpu:-2}"
var_ram="${var_ram:-2048}"
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
FB_DIR="/opt/fetchbox"
FB_UID=1000 # the user the service runs as, inside the container (the install script creates it)
FB_DOWNLOADS="/downloads"

# ------------------------------------------------------------------------------
# Updating (runs inside the container: `update`)
# ------------------------------------------------------------------------------

# The newest published release (its tag), or `main` while there is none. FETCHBOX_REF pins a tag
# or a branch instead.
function fetchbox_target_ref() {
  if [[ -n "${FETCHBOX_REF:-}" ]]; then
    echo "$FETCHBOX_REF"
    return
  fi
  local tag
  # `|| true`: a failing curl (offline, rate limited) must mean "no release", not stop the script.
  tag="$(curl -fsSL -m 15 "https://api.github.com/repos/${FB_REPO}/releases/latest" 2>/dev/null |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1 || true)"
  echo "${tag:-main}"
}

function update_script() {
  header_info
  check_container_storage
  check_container_resources
  if [[ ! -x "${FB_DIR}/fetchbox" ]]; then
    msg_error "No ${APP} Installation Found!"
    exit
  fi

  local ref installed remote
  ref="$(fetchbox_target_ref)"
  installed="$(awk '{print $2}' "${FB_DIR}/.installed" 2>/dev/null || true)"
  remote="$(git ls-remote "https://github.com/${FB_REPO}.git" "${ref}" "${ref}^{}" 2>/dev/null | tail -n1 | cut -f1 || true)"
  if [[ -z "$remote" ]]; then
    msg_error "Could not find '${ref}' on GitHub"
    exit
  fi
  if [[ "$installed" == "$remote" ]]; then
    msg_ok "No update available: ${APP} (${ref})"
    exit
  fi

  msg_info "Installing build tools"
  $STD apt-get install -y git build-essential pkg-config curl ca-certificates
  msg_ok "Installed build tools"
  NODE_VERSION="22" setup_nodejs
  setup_rust

  local src="/opt/fetchbox-src"
  msg_info "Fetching ${APP} ${ref}"
  rm -rf "$src"
  $STD git clone --depth 1 --branch "$ref" "https://github.com/${FB_REPO}.git" "$src"
  msg_ok "Fetched ${APP} ${ref}"

  msg_info "Building the web interface"
  (cd "${src}/web" && $STD npm ci && $STD npm run build)
  msg_ok "Built the web interface"

  msg_info "Building ${APP} (this takes a few minutes)"
  (cd "$src" && $STD cargo build --release -p server)
  msg_ok "Built ${APP}"

  msg_info "Stopping Service"
  systemctl stop fetchbox
  msg_ok "Stopped Service"

  msg_info "Installing the new version"
  install -m 0755 "${src}/target/release/fetchbox" "${FB_DIR}/fetchbox"
  echo "${ref} $(git -C "$src" rev-parse HEAD)" >"${FB_DIR}/.installed"
  msg_ok "Installed the new version"

  msg_info "Cleaning up"
  rm -rf "$src" "$HOME/.cargo" "$HOME/.rustup" "$HOME/.npm"
  $STD apt-get purge -y nodejs build-essential
  $STD apt-get autoremove -y
  msg_ok "Cleaned up"

  msg_info "Starting Service"
  systemctl start fetchbox
  msg_ok "Started Service"
  msg_ok "Updated successfully to ${ref}!"
  exit
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

start
build_container
map_downloads
description

msg_ok "Completed successfully!\n"
echo -e "${CREATING}${GN}${APP} setup has been successfully initialized!${CL}"
echo -e "${INFO}${YW}Open it using the following URL and enter the setup code${CL}"
echo -e "${INFO}${YW}(printed above, or: journalctl -u fetchbox | grep 'setup code'):${CL}"
echo -e "${GATEWAY}${BGN}http://${IP}:8080${CL}"
