#!/usr/bin/env bash
# Installs FetchBox on a Linux host as a systemd service: downloads the release binary, creates a
# service user and the folders, writes the unit and starts it. Nothing is compiled.
#
#   curl -fsSL https://raw.githubusercontent.com/fwitkowski17/WebDownloadManager/main/deploy/install.sh | sudo bash
#   curl -fsSL .../install.sh | sudo bash -s -- install --port 9000 --download-dir /mnt/nas/downloads
#   sudo ./install.sh update
#   sudo ./install.sh uninstall [--purge]
#
# Run `./install.sh help` for all options. Needs root, systemd, curl (or wget), tar and sha256sum.
#
# Layout:
#   /usr/local/bin/fetchbox               the program (the web app is inside it)
#   /etc/fetchbox/fetchbox.env            settings read by the service (kept by update and uninstall)
#   /etc/systemd/system/fetchbox.service  the unit (written by `install`)
#   /var/lib/fetchbox                     the database
#   /var/lib/fetchbox/downloads           downloads, unless --download-dir says otherwise

# The whole script is one function that runs at the very end, so a half-downloaded copy piped into
# bash does nothing.
main() {
  set -euo pipefail

  local REPO="fwitkowski17/WebDownloadManager"
  local RELEASES="${FETCHBOX_RELEASES_URL:-https://github.com/${REPO}/releases}"
  local BIN="/usr/local/bin/fetchbox"
  local CONF_DIR="/etc/fetchbox"
  local ENV_FILE="${CONF_DIR}/fetchbox.env"
  local UNIT="/etc/systemd/system/fetchbox.service"
  local DATA_DIR="/var/lib/fetchbox"

  local action="install" version="latest" port="8080" download_dir="" svc_user="fetchbox"
  local start=1 purge=0
  tmp="" # global: the EXIT trap runs after main has returned

  say() { printf '%s\n' "$*"; }
  warn() { printf 'warning: %s\n' "$*" >&2; }
  die() { printf 'error: %s\n' "$*" >&2; exit 1; }

  usage() {
    cat <<'EOT'
FetchBox installer for a Linux host with systemd.

Usage: install.sh [install|update|uninstall|help] [options]

install (default)
  --version X.Y.Z      release to install (default: the latest)
  --port N             port to listen on (default: 8080)
  --download-dir DIR   where downloads go (default: /var/lib/fetchbox/downloads)
  --user NAME          run as this existing user instead of a new "fetchbox" user, e.g. the
                       owner of a NAS folder
  --no-start           write everything but do not enable or start the service

update
  --version X.Y.Z      release to install (default: the latest)
  Replaces the program and restarts the service. The database, the settings and the unit stay.

uninstall
  --purge              also remove the database, the settings and the service user
  Downloaded files are never removed.
EOT
  }

  # ---- arguments -------------------------------------------------------------------------------
  if [[ $# -gt 0 ]]; then
    case "$1" in
      install | update | uninstall)
        action="$1"
        shift
        ;;
      help | -h | --help)
        usage
        return 0
        ;;
    esac
  fi
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --version)
        [[ $# -ge 2 ]] || die "--version needs a value"
        version="${2#v}"
        shift 2
        ;;
      --port)
        [[ $# -ge 2 ]] || die "--port needs a value"
        port="$2"
        shift 2
        ;;
      --download-dir)
        [[ $# -ge 2 ]] || die "--download-dir needs a value"
        download_dir="$2"
        shift 2
        ;;
      --user)
        [[ $# -ge 2 ]] || die "--user needs a value"
        svc_user="$2"
        shift 2
        ;;
      --no-start)
        start=0
        shift
        ;;
      --purge)
        purge=1
        shift
        ;;
      help | -h | --help)
        usage
        return 0
        ;;
      *) die "unknown option: $1 (see: install.sh help)" ;;
    esac
  done

  if ! [[ "$port" =~ ^[0-9]+$ ]] || ((port < 1 || port > 65535)); then
    die "--port must be a number from 1 to 65535"
  fi
  [[ "$svc_user" =~ ^[a-z_][a-z0-9_-]*$ ]] || die "--user: not a valid user name"
  if [[ -n "$download_dir" ]]; then
    [[ "$download_dir" == /* ]] || die "--download-dir must be an absolute path"
    [[ "$download_dir" =~ ^[A-Za-z0-9_./@+-]+$ ]] || die "--download-dir: use only letters, digits and _ . / @ + - in the path"
  fi
  [[ "$version" == latest || "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "--version must look like 0.7.0"

  [[ "$(id -u)" -eq 0 ]] || die "run as root (sudo)"
  [[ "$(uname -s)" == Linux ]] || die "Linux only"
  command -v systemctl >/dev/null || die "systemd is required (systemctl not found)"

  # A DEB or RPM of FetchBox owns /usr/bin/fetchbox and its own unit: two installs would fight.
  if { command -v dpkg-query >/dev/null && dpkg-query -W fetchbox >/dev/null 2>&1; } ||
    { command -v rpm >/dev/null && rpm -q fetchbox >/dev/null 2>&1; }; then
    die "FetchBox is installed from a package: manage it with apt or dnf (see the README), not with this script"
  fi

  # ---- helpers ---------------------------------------------------------------------------------
  fetch() { # fetch URL FILE
    if command -v curl >/dev/null; then
      curl -fsSL --retry 3 --connect-timeout 15 -o "$2" "$1"
    elif command -v wget >/dev/null; then
      wget -q -O "$2" "$1"
    else
      die "curl or wget is required"
    fi
  }

  asset_name() {
    case "$(uname -m)" in
      x86_64 | amd64) echo "fetchbox-linux-x86_64.tar.gz" ;;
      aarch64 | arm64) echo "fetchbox-linux-aarch64.tar.gz" ;;
      *) die "no release for the CPU $(uname -m) (x86_64 and aarch64 only)" ;;
    esac
  }

  # Downloads the release, checks its checksum and unpacks the program into $tmp/fetchbox.
  download_binary() {
    local asset base
    asset="$(asset_name)"
    if [[ "$version" == latest ]]; then
      base="${RELEASES}/latest/download"
    else
      base="${RELEASES}/download/v${version}"
    fi
    say "Downloading ${asset} (${version})..."
    fetch "${base}/${asset}" "${tmp}/${asset}" || die "cannot download ${base}/${asset} (is there such a release?)"
    fetch "${base}/${asset}.sha256" "${tmp}/${asset}.sha256" || die "cannot download the checksum ${asset}.sha256"
    (cd "$tmp" && sha256sum --check --status "${asset}.sha256") || die "the checksum of ${asset} does not match"
    tar -xzf "${tmp}/${asset}" -C "$tmp" fetchbox || die "cannot unpack ${asset}"
    chmod 0755 "${tmp}/fetchbox"
    # Also proves that this system can run it (glibc, CPU) before anything is replaced.
    "${tmp}/fetchbox" --version >/dev/null 2>&1 || die "the downloaded program does not run on this system"
  }

  new_version() { "${tmp}/fetchbox" --version; }

  place_binary() {
    install -m 0755 "${tmp}/fetchbox" "${BIN}.new"
    mv -f "${BIN}.new" "$BIN"
  }

  unit_user() { # the user the installed unit runs as
    { sed -n 's/^User=//p' "$UNIT" 2>/dev/null || true; } | head -n1
  }

  write_env_file() {
    install -d -m 0755 "$CONF_DIR"
    [[ -e "$ENV_FILE" ]] && return 0 # settings of an earlier install are kept
    cat >"$ENV_FILE" <<'EOT'
# Settings of the FetchBox service (read by systemd). Restart after a change: systemctl restart fetchbox
#
# A fixed password (at least 8 characters). Leave it unset to create the account in the browser
# on first run, with the setup code from `journalctl -u fetchbox`. A password set here wins and
# cannot be changed in the web interface.
#FETCHBOX_PASSWORD=
#
# Log every download, settings change and sign-in to the journal.
#FETCHBOX_LOG_ACTIVITY=true
#
# Log level of everything else, e.g. debug.
#RUST_LOG=info
EOT
    chown "root:${svc_user}" "$ENV_FILE"
    chmod 0640 "$ENV_FILE"
  }

  write_unit() {
    cat >"$UNIT" <<EOT
[Unit]
Description=FetchBox download manager
Documentation=https://github.com/${REPO}
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${svc_user}
Group=${svc_group}
WorkingDirectory=${DATA_DIR}
# Tells the app how it was installed, so its update notice shows the steps of this script.
Environment=FETCHBOX_INSTALL_METHOD=script
EnvironmentFile=-${ENV_FILE}
ExecStart=${BIN} --data-dir ${DATA_DIR} --download-dir ${download_dir} --port ${port}
Restart=on-failure
RestartSec=3

# The service may write to its database and to the downloads, nothing else.
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=read-only
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true
ReadWritePaths=${DATA_DIR} ${download_dir}

[Install]
WantedBy=multi-user.target
EOT
  }

  setup_code() { # the one-time code the first start writes to the journal
    local code="" _
    for _ in $(seq 1 20); do
      code="$(journalctl -u fetchbox --no-pager 2>/dev/null | grep -o 'setup code [A-Z0-9]\{8\}' | tail -n1 | awk '{print $3}' || true)"
      [[ -n "$code" ]] && break
      sleep 1
    done
    printf '%s' "$code"
  }

  # ---- actions ---------------------------------------------------------------------------------
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  case "$action" in
    install)
      if ((start)) && [[ ! -d /run/systemd/system ]]; then
        die "systemd is not running on this host; use --no-start to only write the files"
      fi
      [[ ! -e "$UNIT" && ! -e "$BIN" ]] || die "FetchBox is already installed. To upgrade run: install.sh update"
      [[ -n "$download_dir" ]] || download_dir="${DATA_DIR}/downloads"
      local made_user=0 svc_group
      if id -u "$svc_user" >/dev/null 2>&1; then
        svc_group="$(id -gn "$svc_user")"
        [[ "$svc_user" == fetchbox ]] || say "Running as the existing user ${svc_user}."
      else
        [[ "$svc_user" == fetchbox ]] || die "the user ${svc_user} does not exist"
        command -v useradd >/dev/null || die "useradd not found"
        useradd --system --user-group --home-dir "$DATA_DIR" --no-create-home --shell /usr/sbin/nologin fetchbox
        svc_group="fetchbox"
        made_user=1
      fi

      download_binary
      new="$(new_version)"

      install -d -m 0750 -o "$svc_user" -g "$svc_group" "$DATA_DIR"
      if [[ ! -d "$download_dir" ]]; then
        install -d -m 0755 -o "$svc_user" -g "$svc_group" "$download_dir"
      fi
      if ! runuser -u "$svc_user" -- test -w "$download_dir" 2>/dev/null; then
        warn "${svc_user} cannot write to ${download_dir}: give it access (chown, or an ACL with setfacl) before downloading"
      fi
      place_binary
      write_env_file
      write_unit
      systemctl daemon-reload
      say "Installed ${new}."
      ((made_user)) && say "Created the service user ${svc_user}."

      if ((start)); then
        systemctl enable --now fetchbox
        local code ip
        code="$(setup_code)"
        ip="$(hostname -I 2>/dev/null | awk '{print $1}')"
        say ""
        say "FetchBox is running: http://${ip:-<this host>}:${port}"
        if [[ -n "$code" ]]; then
          say "Setup code (enter it in the web interface to create the account): ${code}"
        else
          say "Setup code: journalctl -u fetchbox | grep 'setup code'"
        fi
        say "Settings: ${ENV_FILE}   Logs: journalctl -u fetchbox -f"
      else
        say "Not started. To start it: systemctl enable --now fetchbox"
      fi
      ;;

    update)
      [[ -x "$BIN" && -e "$UNIT" ]] || die "FetchBox is not installed here. To install it run: install.sh"
      download_binary
      new="$(new_version)"
      old="$("$BIN" --version 2>/dev/null || true)"
      if [[ "$new" == "$old" ]]; then
        say "Already up to date (${old})."
        return 0
      fi
      local was_active=0
      systemctl is-active --quiet fetchbox && was_active=1
      ((was_active)) && systemctl stop fetchbox
      place_binary
      # A unit written by an earlier version of this script does not tell the app how it was
      # installed; add that one line (nothing else of the unit is touched).
      if ! grep -q '^Environment=FETCHBOX_INSTALL_METHOD=' "$UNIT"; then
        sed -i '/^EnvironmentFile=/i Environment=FETCHBOX_INSTALL_METHOD=script' "$UNIT"
        systemctl daemon-reload
      fi
      ((was_active)) && systemctl start fetchbox
      say "Updated ${old:-unknown} -> ${new}. The database and the settings were not touched."
      ;;

    uninstall)
      [[ -e "$UNIT" || -e "$BIN" || -e "$CONF_DIR" || -d "$DATA_DIR" ]] || die "FetchBox is not installed here"
      svc_user="$(unit_user)"
      svc_user="${svc_user:-fetchbox}"
      if [[ -e "$UNIT" ]]; then
        systemctl disable --now fetchbox 2>/dev/null || true
      fi
      rm -f "$UNIT" "$BIN"
      systemctl daemon-reload
      if ((purge)); then
        rm -rf "$DATA_DIR/fetchbox.db" "$DATA_DIR/fetchbox.db-wal" "$DATA_DIR/fetchbox.db-shm" "$CONF_DIR"
        rmdir "$DATA_DIR" 2>/dev/null || true
        # Only the user this script created.
        if [[ "$svc_user" == fetchbox ]] && id -u fetchbox >/dev/null 2>&1; then
          userdel fetchbox 2>/dev/null || true
        fi
        say "Removed the service, the program, the database and the settings. Downloaded files were left alone."
      else
        say "Removed the service and the program. Kept: ${DATA_DIR} (database), ${CONF_DIR} (settings) and all downloads."
        say "Remove those too with: install.sh uninstall --purge"
      fi
      ;;
  esac
}

main "$@"
