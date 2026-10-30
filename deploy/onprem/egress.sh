#!/bin/sh
# Caps this container's outgoing traffic, so an instance cannot be used to
# flood others. Runs at container start, as root with NET_ADMIN, before the
# entrypoint drops every capability.
#
# BEEP_EGRESS_LIMIT is in bytes per second: a number, or one with K, M or G
# for KiB, MiB or GiB per second (1M is 1 MiB/s). Empty means the default,
# 1M. `0` or `unlimited` turns the cap off. BEEP_EGRESS_DEVICE names the
# interface (default eth0). With BEEP_EGRESS_DRY_RUN=1 it prints the tc
# command instead of running it.
#
# Exits non-zero when the cap cannot be applied, so the container does not
# start uncapped by mistake.
set -eu

limit=${BEEP_EGRESS_LIMIT:-1M}
device=${BEEP_EGRESS_DEVICE:-eth0}

fail() {
    echo "beep-egress: $*" >&2
    exit 1
}

case "$limit" in
    0 | unlimited)
        echo "beep-egress: outgoing traffic is not capped"
        exit 0
        ;;
esac

number=${limit%[KkMmGg]}
case "$number" in
    '' | *[!0-9]*) fail "BEEP_EGRESS_LIMIT must be bytes per second like 1048576 or 1M, or unlimited; not $limit" ;;
esac
# Strip leading zeros so the shell does not read the number as octal.
number=$(printf '%s' "$number" | sed 's/^0*//')
[ -n "$number" ] || fail "BEEP_EGRESS_LIMIT is zero; write 0 or unlimited for no cap"
case "$limit" in
    *[Kk]) bytes=$((number * 1024)) ;;
    *[Mm]) bytes=$((number * 1024 * 1024)) ;;
    *[Gg]) bytes=$((number * 1024 * 1024 * 1024)) ;;
    *) bytes=$number ;;
esac
# tc's rates are clearest in bits; the bucket holds a quarter second of
# traffic, at least 16 KiB so small rates still pass full packets.
bits=$((bytes * 8))
burst=$((bytes / 4))
[ "$burst" -ge 16384 ] || burst=16384

set -- tc qdisc replace dev "$device" root tbf rate "${bits}bit" burst "$burst" latency 200ms
if [ "${BEEP_EGRESS_DRY_RUN:-}" = 1 ]; then
    echo "$*"
    exit 0
fi
command -v tc > /dev/null || fail "tc is missing; the image needs iproute2"
if ! "$@"; then
    fail "could not cap outgoing traffic on $device (the container needs NET_ADMIN). Refusing to start; set BEEP_EGRESS_LIMIT=unlimited to run without a cap."
fi
echo "beep-egress: outgoing traffic on $device capped at $bytes bytes/s"
