#!/bin/sh
# Checks how egress.sh reads BEEP_EGRESS_LIMIT, without touching the network.
# Run: sh deploy/onprem/egress-test.sh
set -u
script="$(dirname "$0")/egress.sh"
failed=0

expect() {
    want_status=$1 want_text=$2 limit=$3
    got=$(BEEP_EGRESS_DRY_RUN=1 BEEP_EGRESS_LIMIT="$limit" sh "$script" 2>&1)
    status=$?
    case "$got" in
        *"$want_text"*) text_ok=1 ;;
        *) text_ok=0 ;;
    esac
    if [ "$status" != "$want_status" ] || [ "$text_ok" != 1 ]; then
        echo "FAIL limit='$limit': status $status, output: $got"
        failed=1
    fi
}

expect 0 "rate 8388608bit burst 262144" ""
expect 0 "rate 8388608bit burst 262144" 1M
expect 0 "rate 8388608bit" 1m
expect 0 "rate 8388608bit" 1048576
expect 0 "rate 819200bit burst 25600" 100K
expect 0 "rate 8000bit burst 16384" 1000
expect 0 "rate 8589934592bit" 1G
expect 0 "rate 8388608bit" 01M
expect 0 "not capped" 0
expect 0 "not capped" unlimited
expect 1 "must be bytes per second" 1mbit
expect 1 "must be bytes per second" -5
expect 1 "must be bytes per second" M
expect 1 "must be bytes per second" "1 M"
expect 1 "is zero" 00

[ "$failed" = 0 ] && echo "egress.sh: all cases pass"
exit "$failed"
