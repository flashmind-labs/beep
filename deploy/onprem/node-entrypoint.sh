#!/bin/sh
# Starts the bundled node: caps outgoing traffic (see egress.sh), then runs
# beep-node as `beep` with no capabilities, so the commands it runs cannot
# lift the cap.
set -eu
beep-egress
if [ "$(id -u)" = 0 ]; then
    exec setpriv --reuid beep --regid beep --init-groups \
        --inh-caps=-all --bounding-set=-all \
        env HOME=/home/beep beep-node "$@"
fi
exec beep-node "$@"
