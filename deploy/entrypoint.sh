#!/bin/sh
# Starts the server as `beep` with no capabilities. Root and CAP_SYS_ADMIN are
# only needed here: Docker bind-mounts single files over /etc, and Drop cannot
# overlay an /etc with mounts locked inside it, so they are replaced by copies.
set -eu
if [ "$(id -u)" = 0 ]; then
    for f in /etc/hosts /etc/resolv.conf /etc/hostname; do
        if mountpoint -q "$f"; then
            content=$(cat "$f")
            umount "$f"
            printf '%s\n' "$content" > "$f"
        fi
    done
    exec setpriv --reuid beep --regid beep --init-groups \
        --inh-caps=-all --bounding-set=-all \
        env HOME=/home/beep beep-server "$@"
fi
exec beep-server "$@"
