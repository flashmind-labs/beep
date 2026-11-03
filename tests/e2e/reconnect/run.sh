#!/usr/bin/env bash
# A node that stops in the middle of a sync picks it up again.
#
# Topology as in the sync scenario: server, node-a and node-b, one project
# synced on both nodes.
#
# node-a's outgoing traffic is capped with tc so a large file takes a while
# to cross. While node-b is fetching it (its blob store holds part of the
# file), node-b's process is stopped:
#   1. with SIGTERM, then started again
#   2. with SIGKILL, then started again
# Each time the file must arrive whole on node-b (same sha256), with no
# partial folder left behind, and the server must report both in sync.
set -uo pipefail

E2E="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$E2E/../.." && pwd)"
source "$E2E/lib/common.sh"
source "$E2E/lib/beep.sh"

SIZE_MB="${E2E_RESUME_MB:-200}"
RATE="${E2E_RESUME_RATE:-80mbit}"
# Expanded on the VM, also inside quotes there.
P="\$HOME/ws/$PROJECT"

# The interfaces holding node-a's IPv4 and IPv6 default routes: iroh may
# send over either, or through a relay over either.
UPLINKS="{ ip -4 route show default; ip -6 route show default; } | awk '{for(i=1;i<NF;i++) if(\$i==\"dev\") print \$(i+1)}' | sort -u"

# cap / uncap : limit node-a's egress on every uplink.
cap(){
  on node-a "for dev in \$($UPLINKS); do sudo tc qdisc replace dev \$dev root tbf rate $RATE burst 256kb latency 400ms || exit 1; done"
}
uncap(){
  on node-a "for dev in \$($UPLINKS); do sudo tc qdisc del dev \$dev root 2> /dev/null; done; true"
}

# fetching_mb : MB of the blob node-b received last. A fetch lands in the
# node's blob store first and is copied into the folder once whole.
fetching_mb(){
  on node-b "f=\$(ls -t ~/beep/state/blobs/data/*.data 2> /dev/null | head -1)
if [ -n \"\$f\" ]; then du -m \"\$f\" | cut -f1; else echo 0; fi"
}

# halfway : node-b has fetched between a tenth and most of the file.
halfway(){
  local mb; mb="$(fetching_mb)" || return 1
  (( mb >= SIZE_MB / 10 && mb < SIZE_MB * 8 / 10 ))
}

no_partial(){ on node-b "! ls -a $P | grep -q '^\.beep-.*\.partial\$'"; }

# interrupt <n> <signal> : a new large file on node-a; stop node-b with
# <signal> while it is fetching, start it, and check the file arrives.
interrupt(){
  local n="$1" signal="$2"
  step "$n. node-b stopped with SIG$signal mid-fetch, then started"
  on node-a "head -c ${SIZE_MB}M /dev/urandom > $P/large-$n.bin" || abort "could not write on node-a"
  if ! wait_for 180 "node-b fetching large-$n.bin" halfway; then
    fail "node-b never got partway through large-$n.bin (at $(fetching_mb) MB)"
    return
  fi
  note "stopping node-b at $(fetching_mb) MB of $SIZE_MB"
  stop_node node-b "$signal"
  start_node node-b
  synced 300 "large-$n.bin complete on node-b after SIG$signal"
  if no_partial; then pass "no partial folder left on node-b"; else fail "a partial folder is left on node-b"; fi
}

fresh_world
trap uncap EXIT
turn_on_sync
on node-a "echo seed > $P/seed.txt"
synced 120 "a first file on node-b"
cap && note "node-a's egress capped at $RATE"

interrupt 1 TERM
interrupt 2 KILL

summary
