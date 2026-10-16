#!/bin/sh
# Installs a FetchBox package in a throw-away system (a container without systemd), checks what the
# installation left behind and removes it again:
#
#   packaging/smoke-test.sh <file.deb|file.rpm> <version> [--run]
#
# With --run the installed program is also started and must answer /api/health (needs a real
# binary; the CI uses a stub without --run). Run it as root, in a container: it installs and removes
# packages, creates a user and deletes /var/lib/fetchbox.
set -eu

[ $# -ge 2 ] || {
  sed -n '2,9p' "$0" >&2
  exit 2
}
pkg="$(realpath "$1")"
version="$2"
run="${3:-}"

ok() { echo "ok: $*"; }
fail() {
  echo "FAIL: $*" >&2
  exit 1
}

case "$pkg" in
  *.deb)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq "$pkg" >/dev/null
    ;;
  *.rpm) dnf install -y -q "$pkg" >/dev/null ;;
  *) fail "not a .deb or .rpm: $pkg" ;;
esac

[ "$(fetchbox --version)" = "fetchbox ${version}" ] || fail "fetchbox --version says '$(fetchbox --version)', expected 'fetchbox ${version}'"
ok "program installed: $(fetchbox --version)"
[ -f /usr/lib/systemd/system/fetchbox.service ] || fail "no systemd unit"
ok "systemd unit installed"
[ "$(stat -c %a /etc/fetchbox/fetchbox.env)" = 600 ] || fail "fetchbox.env is not 0600"
ok "settings file installed (0600)"
getent passwd fetchbox >/dev/null || fail "no user fetchbox"
[ "$(stat -c '%U %a' /var/lib/fetchbox)" = "fetchbox 750" ] || fail "/var/lib/fetchbox is '$(stat -c '%U %a' /var/lib/fetchbox)', expected 'fetchbox 750'"
ok "user fetchbox and /var/lib/fetchbox in place"

# Makes sure a command the test needs exists, installing the package that has it: a minimal
# container image may lack it (the Fedora one has no runuser).
need() {
  command -v "$1" >/dev/null && return 0
  case "$pkg" in
    *.deb) apt-get install -y -qq "$2" >/dev/null ;;
    *) dnf install -y -q "$2" >/dev/null ;;
  esac
  command -v "$1" >/dev/null || fail "$1 is not available, and installing $2 did not help"
}

# What --run needs. Checked every time, so that the CI, which cannot run its stub program, still
# proves that these tools can be had on each system.
need curl curl
need runuser util-linux
ok "curl and runuser are available"

if [ "$run" = "--run" ]; then
  # The same settings the unit gives the service, on another port.
  runuser -u fetchbox -- env FETCHBOX_DATA_DIR=/var/lib/fetchbox FETCHBOX_DOWNLOAD_DIR=/var/lib/fetchbox/downloads \
    FETCHBOX_PORT=18080 fetchbox >/tmp/fetchbox-smoke.log 2>&1 &
  pid=$!
  up=0
  for _ in $(seq 1 30); do
    if curl -fsS http://127.0.0.1:18080/api/health >/dev/null 2>&1; then
      up=1
      break
    fi
    sleep 1
  done
  if [ "$up" != 1 ]; then
    cat /tmp/fetchbox-smoke.log >&2
    fail "the installed program did not answer /api/health"
  fi
  [ -d /var/lib/fetchbox/downloads ] || fail "the downloads folder was not created"
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  ok "the program starts as user fetchbox and answers /api/health"
fi

case "$pkg" in
  *.deb)
    apt-get remove -y -qq fetchbox >/dev/null
    [ ! -e /usr/bin/fetchbox ] || fail "the program is still there after remove"
    [ -f /etc/fetchbox/fetchbox.env ] || fail "remove deleted the settings (only purge may)"
    apt-get purge -y -qq fetchbox >/dev/null
    [ ! -e /etc/fetchbox/fetchbox.env ] || fail "purge left the settings"
    ;;
  *.rpm)
    dnf remove -y -q fetchbox >/dev/null
    [ ! -e /usr/bin/fetchbox ] || fail "the program is still there after erase"
    ;;
esac
[ -d /var/lib/fetchbox ] || fail "removing the package deleted /var/lib/fetchbox (the database)"
ok "removed; the data in /var/lib/fetchbox is kept"
