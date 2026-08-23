#!/bin/sh
set -eu

# A Linux bind mount created by Docker is normally owned by root. Fix ownership
# before dropping privileges so a freshly downloaded Compose file works without
# host-specific UID commands and a packed ./data directory remains portable.
if [ "$(id -u)" -eq 0 ]; then
  mkdir -p /app/data/storage
  owner_marker=/app/data/.chronoframe-owner-v1
  if [ ! -f "$owner_marker" ] || [ "$(stat -c '%u:%g' "$owner_marker")" != "10001:10001" ]; then
    chown -R chronoframe:chronoframe /app/data
    touch "$owner_marker"
    chown chronoframe:chronoframe "$owner_marker"
  fi
  exec gosu chronoframe "$@"
fi

exec "$@"
