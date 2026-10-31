#!/bin/sh
# Starts the server as `beep` with no capabilities.
set -eu
if [ "$(id -u)" = 0 ]; then
    exec setpriv --reuid beep --regid beep --init-groups \
        --inh-caps=-all --bounding-set=-all \
        env HOME=/home/beep beep-server "$@"
fi
exec beep-server "$@"
