#!/bin/sh
# aardbin entrypoint — ensures the bind-mounted data dir is writable by the
# non-root `aardbin` user, then drops privileges (SPEC §35.2 bind mount).
set -e

BIN=/usr/local/bin/aardbin

if [ "$(id -u)" = "0" ]; then
    mkdir -p /app/data
    chown -R aardbin:aardbin /app/data
    exec setpriv --reuid=aardbin --regid=aardbin --init-groups "$BIN" "$@"
fi

exec "$BIN" "$@"
