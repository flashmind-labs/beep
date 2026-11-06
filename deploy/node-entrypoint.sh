#!/bin/sh
# Starts the node-image container's beep-node, its key and mesh kept in
# /data. A node not in a mesh yet joins the one BEEP_TICKET names, read from
# stdin so the ticket shows in no process listing or log.
set -eu
dir=/data/beep-node
mkdir -p "$dir"
ticket=${BEEP_TICKET:-}
unset BEEP_TICKET

if [ -f "$dir/mesh2/devices.bin" ]; then
    exec beep-node --dir "$dir" run
fi
if [ -z "$ticket" ]; then
    echo "beep-node is in no mesh. Set BEEP_TICKET to a ticket from the desktop's Add a machine to join one." >&2
    exec beep-node --dir "$dir" run
fi

# `mesh join` hands the ticket to a running node: wait for its socket. One
# left by an earlier container is gone first.
rm -f "$dir/mesh.sock"
beep-node --dir "$dir" run &
node=$!
trap 'kill -TERM "$node" 2>/dev/null || true' TERM INT
tries=0
while [ ! -S "$dir/mesh.sock" ] && kill -0 "$node" 2>/dev/null && [ "$tries" -lt 300 ]; do
    sleep 0.1
    tries=$((tries + 1))
done
if ! printf '%s' "$ticket" | beep-node --dir "$dir" mesh join -; then
    echo "beep-node could not join the mesh. A ticket works once: make a new one if it was used." >&2
    kill -TERM "$node" 2>/dev/null || true
    wait "$node" || true
    exit 1
fi
status=0
wait "$node" || status=$?
# A signal ends the first wait early: wait for the node to stop.
while kill -0 "$node" 2>/dev/null; do
    status=0
    wait "$node" || status=$?
done
exit "$status"
