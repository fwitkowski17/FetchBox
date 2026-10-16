#!/bin/sh
# deb: $1 = configure, $2 = the previous version (empty on a first install)
# rpm: $1 = number of installed versions after this one (1 = first install, 2 = upgrade)
set -e

upgrade=0
case "$1" in
  configure) [ -n "${2:-}" ] && upgrade=1 ;;
  2) upgrade=1 ;;
esac

# The service user. It may exist already (the Proxmox installer makes it with a fixed uid).
if ! getent passwd fetchbox >/dev/null 2>&1; then
  if command -v useradd >/dev/null 2>&1; then
    useradd --system --user-group --home-dir /var/lib/fetchbox --no-create-home --shell /usr/sbin/nologin fetchbox
  else
    adduser --system --group --home /var/lib/fetchbox --no-create-home --shell /usr/sbin/nologin fetchbox
  fi
fi
group="$(id -gn fetchbox)"
install -d -m 0750 -o fetchbox -g "$group" /var/lib/fetchbox

# No systemd (a container image build, a chroot): the files are in place, nothing to start.
[ -d /run/systemd/system ] || exit 0

systemctl daemon-reload || true
if [ "$upgrade" = 1 ]; then
  # Only a running service is restarted; one that the administrator stopped stays stopped.
  systemctl try-restart fetchbox || true
  exit 0
fi

systemctl enable fetchbox >/dev/null 2>&1 || true
systemctl start fetchbox || true

# The first start prints a one-time code that creates the account in the browser.
code=""
for _ in 1 2 3 4 5 6 7 8; do
  code="$(journalctl -u fetchbox --no-pager 2>/dev/null | grep -o 'setup code [A-Z0-9]\{8\}' | tail -n 1 | cut -d ' ' -f 3 || true)"
  [ -n "$code" ] && break
  sleep 1
done
echo "FetchBox is running on port 8080 (settings: /etc/fetchbox/fetchbox.env)."
if [ -n "$code" ]; then
  echo "Open it in a browser and enter the setup code: $code"
else
  echo "Open it in a browser; the setup code: journalctl -u fetchbox | grep 'setup code'"
fi
