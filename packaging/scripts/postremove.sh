#!/bin/sh
# The database, the downloads, /etc/fetchbox/fetchbox.env (when edited) and the user "fetchbox" stay
# on purpose; see the README to remove them.
set -e
if [ -d /run/systemd/system ]; then
  systemctl daemon-reload || true
fi
