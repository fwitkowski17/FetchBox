#!/bin/sh
# deb: $1 = remove | upgrade | deconfigure | failed-upgrade;  rpm: $1 = 0 (erase) or 1 (upgrade)
set -e
case "$1" in
  remove | 0)
    if [ -d /run/systemd/system ]; then
      systemctl disable --now fetchbox >/dev/null 2>&1 || true
    fi
    ;;
esac
