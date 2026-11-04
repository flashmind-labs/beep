#!/usr/bin/env bash
# A large transfer that is interrupted resumes where it stopped.
#
# Topology as in the sync scenario: server, node-a and node-b, one project
# synced on both nodes. node-a's egress is capped with tc so a 500 MB file
# takes about a minute to cross. While node-b is partway through fetching
# one (a fifth to three fifths of it in its blob store):
#   1. node-b, the receiver, is restarted (SIGTERM, then started)
#   2. node-a, the sender, is restarted
#   3. node-b's network is cut with iptables DROP for 30 s, every process
#      left running, then restored
# Each time:
#   - what node-b receives after the interruption (bytes on its uplinks)
#     must be about what was left, well under the file: a resume, not a
#     new start
#   - the file arrives on node-b with node-a's sha256
#   - no partial folder or journal entry is left on either node
#   - the server reports both in sync
set -uo pipefail

E2E="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$E2E/../.." && pwd)"
source "$E2E/lib/common.sh"
source "$E2E/lib/beep.sh"

SIZE_MB="${E2E_INTERRUPT_MB:-500}"
RATE="${E2E_INTERRUPT_RATE:-80mbit}"
CUT_SECS="${E2E_CUT_SECS:-30}"
# Expanded on the VM, also inside quotes there.
P="\$HOME/ws/$PROJECT"

# partway : node-b has fetched a fifth to three fifths of the file.
partway(){
  local mb; mb="$(fetching_mb node-b)" || return 1
  (( mb >= SIZE_MB / 5 && mb < SIZE_MB * 3 / 5 ))
}

# The rules that cut a VM off: everything but ssh (port 22, how the test
# reaches it) and loopback is dropped, both ways, over IPv4 and IPv6.
CUT="for t in iptables ip6tables; do
  sudo \$t -N e2e-cut && sudo \$t -A e2e-cut -p tcp --dport 22 -j RETURN \
  && sudo \$t -A e2e-cut -p tcp --sport 22 -j RETURN && sudo \$t -A e2e-cut -j DROP \
  && sudo \$t -I INPUT 1 ! -i lo -j e2e-cut && sudo \$t -I OUTPUT 1 ! -o lo -j e2e-cut || exit 1
done"
RESTORE="for t in iptables ip6tables; do
  sudo \$t -D INPUT ! -i lo -j e2e-cut; sudo \$t -D OUTPUT ! -o lo -j e2e-cut
  sudo \$t -F e2e-cut; sudo \$t -X e2e-cut
done 2> /dev/null; true"

# cut_network <role> : drop the VM's traffic for $CUT_SECS s. The VM restores
# it by itself: `boat ssh` asks the Boat API first, which can time out for a
# VM that is cut off, and that must not make the cut longer.
cut_network(){
  on "$1" "(sleep $CUT_SECS; $RESTORE) > /dev/null 2>&1 < /dev/null & $CUT" \
    || { on "$1" "$RESTORE"; fail "could not cut $1's network"; return 1; }
  note "$1's network cut for $CUT_SECS s"
  sleep "$CUT_SECS"
  if poll 120 on "$1" "! sudo iptables -S e2e-cut 2> /dev/null"; then
    note "$1's network restored"
  else
    fail "$1's network was not restored"
  fi
}

restart(){ stop_node "$1" TERM && start_node "$1"; }

# interrupt <n> <what> <command...> : a new large file on node-a; run the
# command while node-b is partway through it, then check it resumes.
interrupt(){
  local n="$1" what="$2"; shift 2
  local name="large-$n.bin" at rx0 rx1 after bound t0 secs
  step "$n. $what mid-transfer"
  drop_file node-a "$name" "$SIZE_MB" || abort "could not write on node-a"
  if ! wait_for 300 "node-b partway through $name" partway; then
    fail "node-b never got partway through $name (at $(fetching_mb node-b) MB)"
    return
  fi
  rx0="$(rx_bytes node-b)"
  at="$(fetching_mb node-b)"
  t0="$(now)"
  note "interrupting at $at MB of $SIZE_MB"
  "$@"
  if ! poll 900 on node-b "test -f ~/ws/$PROJECT/$name"; then
    fail "$name did not reach node-b after: $what"
    probe sync-status "$PROJECT" | sed 's/^/   /'
    return
  fi
  secs="$(since "$t0")"
  rx1="$(rx_bytes node-b)"
  after=$(( (rx1 - rx0) / 1048576 ))
  # What was left, and a tenth of the file for protocol and other traffic.
  bound=$(( SIZE_MB - at + SIZE_MB / 10 ))
  note "node-b received $after MB after the interruption; $((SIZE_MB - at)) MB were left"
  if (( after <= bound )); then
    pass "resumed: $after MB received after the interruption, at most $bound MB expected"
  else
    fail "started over: $after MB received after the interruption, more than $bound MB"
  fi
  measure "$what: MB after (of $SIZE_MB)" "$after" MB max ""
  measure "$what: s to arrive" "$secs" s max ""
  if same_file "$name"; then pass "$name on node-b with node-a's sha256"; else fail "$name differs on node-b"; fi
  local role found
  for role in node-a node-b; do
    found="$(leftovers "$role")"
    if [[ -z "$found" ]]; then pass "no partial files on $role"; else
      fail "partial files left on $role"
      sed 's/^/   /' <<< "$found"
    fi
  done
  wait_for 120 "both in sync" in_sync && pass "the server sees both in sync" \
    || { fail "the server does not see both in sync"; probe sync-status "$PROJECT" | sed 's/^/   /'; }
}

fresh_world
put_back(){ uncap node-a; on node-b "$RESTORE"; }
trap put_back EXIT
turn_on_sync
on node-a "echo seed > $P/seed.txt"
synced 120 "a first file on node-b"
cap node-a "$RATE" && note "node-a's egress capped at $RATE"

interrupt 1 "receiver restarted" restart node-b
interrupt 2 "sender restarted" restart node-a
interrupt 3 "receiver's network cut ${CUT_SECS} s" cut_network node-b

summary
