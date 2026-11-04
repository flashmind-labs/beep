#!/usr/bin/env bash
# A power cut on a node while it takes in a tree. The reboot loses all the VM's
# file data that was not flushed, so a file renamed into place before its
# content reached the disk comes back empty, like after a real cut.
#
# For each cut, node-a makes a tree and node-b reboots partway through it:
#   1. every file at its path on node-b after the reboot is whole: what node-a
#      holds there. A file not there yet is fine.
#   2. node-b starts again and ends up with node-a's manifest, node-a's stays
#      as it was, and neither made a conflict copy: nothing node-b lost was
#      taken for an edit of its own.
set -uo pipefail

E2E="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$E2E/../.." && pwd)"
source "$E2E/lib/common.sh"
source "$E2E/lib/beep.sh"

CUTS="${E2E_POWERCUT_ROUNDS:-3}"
SMALL="${E2E_POWERCUT_FILES:-4000}"
MB_FILES="${E2E_POWERCUT_MB_FILES:-40}"
WANT=$((SMALL + MB_FILES))
LIMIT=900
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# manifest <role> <dir> : the sorted "sha256  path" of every file under it.
manifest(){
  on "$1" "cd ~/ws/$PROJECT && find $2 -type f -print0 2> /dev/null | LC_ALL=C sort -z | xargs -0 -r sha256sum"
}
count(){ on "$1" "find ~/ws/$PROJECT/$2 -type f 2> /dev/null | wc -l"; }
boot_id(){ on "$1" 'cat /proc/sys/kernel/random/boot_id'; }
rebooted(){ [[ "$(boot_id "$1")" != "$2" && -n "$(boot_id "$1")" ]]; }
holds(){ [[ "$(manifest "$1" "$2" | sha256sum)" == "$3" ]]; }
conflicts(){ on "$1" "find ~/ws/$PROJECT -name '*(conflict*' | wc -l"; }

fresh_world
turn_on_sync
put node-a "$E2E/scale/make-tree.sh" beep/make-tree.sh || abort "could not copy make-tree.sh"

for ((cut = 1; cut <= CUTS; cut++)); do
  dir="tree-$cut"
  # Each cut lands further into its tree.
  at=$((WANT * cut / (CUTS + 1)))
  step "cut $cut of $CUTS: node-b reboots at $at of $WANT files of $dir"
  on node-a "bash ~/beep/make-tree.sh ~/ws/$PROJECT/$dir $SMALL $MB_FILES" || abort "could not make $dir"
  manifest node-a "$dir" > "$WORK/want"
  [[ "$(wc -l < "$WORK/want")" == "$WANT" ]] || abort "node-a holds no whole $dir"
  started=$SECONDS
  while (( $(count node-b "$dir" || echo 0) < at )); do
    (( SECONDS - started > LIMIT )) && abort "node-b did not reach $at files of $dir"
    sleep 0.2
  done
  boot="$(boot_id node-b)"
  # The ssh goes with the VM, so the reboot waits for it to return.
  on node-b "nohup sh -c 'sleep 0.5; echo b | sudo tee /proc/sysrq-trigger' > /dev/null 2>&1 &"
  wait_for 300 "node-b is back" rebooted node-b "$boot" || abort "node-b did not come back"

  manifest node-b "$dir" > "$WORK/got"
  bad="$(LC_ALL=C comm -23 <(LC_ALL=C sort "$WORK/got") <(LC_ALL=C sort "$WORK/want"))"
  note "node-b kept $(wc -l < "$WORK/got") of $WANT files through the cut"
  if [[ -z "$bad" ]]; then
    pass "cut $cut: every file on node-b is whole"
  else
    fail "cut $cut: $(wc -l <<< "$bad") files on node-b are not what node-a holds"
    head -5 <<< "$bad" | sed 's/^/   /'
  fi

  start_node node-b
  digest="$(sha256sum < "$WORK/want")"
  wait_for "$LIMIT" "node-b holds $dir" holds node-b "$dir" "$digest" \
    && pass "cut $cut: node-b ends up with all of $dir" \
    || fail "cut $cut: node-b does not hold all of $dir"
  holds node-a "$dir" "$digest" \
    && pass "cut $cut: node-a still holds what it made" \
    || fail "cut $cut: node-a's $dir changed"
  copies="$(conflicts node-a) $(conflicts node-b)"
  if [[ "$copies" == "0 0" ]]; then pass "cut $cut: no conflict copies"; else
    fail "cut $cut: conflict copies on node-a and node-b: $copies"
  fi
  wait_for 120 "both in sync" in_sync && pass "cut $cut: the server sees both in sync" \
    || { fail "cut $cut: the server does not see both in sync"; probe sync-status "$PROJECT" | sed 's/^/   /'; }
done

summary
