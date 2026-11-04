#!/usr/bin/env bash
# File sync between two nodes, over iroh-blobs.
#
# Topology:
#   server  beep-server and Postgres; beep-probe plays the phone
#   node-a  beep-node, workspace ~/ws, project folder ~/ws/e2e
#   node-b  the same, on another VM
#
# The phone links both nodes with setup tokens and turns on the project's
# sync on each, as the app does (SetProjectSync). Then, checking after each
# step that node-b holds exactly node-a's files (sha256 of every file) and
# that the server reports both machines in sync:
#   1. new files on node-a: small ones, a nested tree, a large random file,
#      timed to the first file and to all of them on node-b; then a
#      200 MB file timed for MB/s
#   2. edits (a small file, the middle of the large one), a deleted file, a
#      deleted folder, a renamed file and a renamed folder
#   3. a file written on node-b reaches node-a
#   4. a conflicting edit: node-b is stopped, both edit one file, node-b
#      starts. The newer edit keeps the path on both; the other becomes a
#      `name (conflict xxxxxxxx).ext` copy beside it on both.
set -uo pipefail

E2E="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$E2E/../.." && pwd)"
source "$E2E/lib/common.sh"
source "$E2E/lib/beep.sh"

BIG_MB="${E2E_BIG_MB:-48}"
SPEED_MB="${E2E_SPEED_MB:-200}"
# Expanded on the VM, also inside quotes there.
P="\$HOME/ws/$PROJECT"

fresh_world
turn_on_sync

# Whether node-a's own list holds files: its scan saw them.
listed(){
  probe sync-status "$PROJECT" | awk -v id="${MACHINE[node-a]}" '$1 == id && /files=[1-9]/ { ok = 1 } END { exit !ok }'
}

step "1. new files on node-a reach node-b"
t0="$(now)"
on node-a "cd $P && printf 'hello\n' > hello.txt && printf 'first\n' > notes.txt
mkdir -p tree/a/b/c tree/d
for i in \$(seq 1 40); do head -c \$((i * 1000)) /dev/urandom > tree/a/file-\$i.dat; done
for i in \$(seq 1 10); do echo \"deep \$i\" > tree/a/b/c/deep-\$i.txt; done
echo spaced > 'tree/d/with space.txt'
head -c ${BIG_MB}M /dev/urandom > big.bin" || abort "could not write on node-a"
# Apart from the first file arriving, so a slow first file shows whether the
# scan or the way to the peer took the time.
if poll 300 listed; then
  measure "sync: in node-a's list" "$(since "$t0")" s max "${E2E_BUDGET_SYNC_LISTED-30}"
fi
if poll 300 on node-b "find $P -type f ! -name '.beep-*' | grep -q ."; then
  measure "sync: first file" "$(since "$t0")" s max "${E2E_BUDGET_SYNC_FIRST-30}"
fi
if poll 300 same_files; then
  measure "sync: all files" "$(since "$t0")" s max "${E2E_BUDGET_SYNC_ALL-45}"
fi
synced 60 "${BIG_MB} MB file, nested tree and small files on node-b"

step "1b. a ${SPEED_MB} MB file, node-a to node-b"
file_rate node-a node-b speed.bin "$SPEED_MB" "sync: $SPEED_MB MB file, MB/s" "${E2E_BUDGET_SYNC_MBPS-6}"
synced 60 "node-b holds node-a's files"

step "2. edits, deletes and renames on node-a"
on node-a "cd $P && printf 'second\n' > hello.txt
dd if=/dev/urandom of=big.bin bs=1M count=1 seek=10 conv=notrunc status=none
rm tree/a/file-7.dat
rm -r tree/a/b
mv tree/a/file-1.dat tree/renamed.dat
mv tree/d tree/moved" || abort "could not change node-a"
synced 180 "node-b follows edits, deletes and renames"
gone(){ on node-b "! test -e $P/tree/a/b && ! test -e $P/tree/d && test -e \"$P/tree/moved/with space.txt\""; }
if gone; then pass "deleted and renamed folders are gone on node-b"; else fail "old folders left on node-b"; fi

step "3. a file written on node-b reaches node-a"
on node-b "cd $P && echo 'from b' > from-b.txt" || abort "could not write on node-b"
synced 120 "node-b's file on node-a"

step "4. a conflicting edit on both nodes"
stop_node node-b
on node-a "cd $P && echo 'edit on a' > notes.txt"
# node-b edits a few seconds later, so its edit is the newer by mtime even
# if the two VMs' clocks differ by a second.
sleep 3
on node-b "cd $P && echo 'edit on b' > notes.txt"
start_node node-b
conflict_settled(){
  same_files && [[ "$(on node-a "ls $P | grep -c '^notes (conflict [0-9a-f]*).txt\$'")" == 1 ]]
}
if wait_for 180 "the conflict settled on both nodes" conflict_settled; then
  pass "both nodes hold notes.txt and one conflict copy"
  kept_a="$(on node-a "cat $P/notes.txt")"
  kept_b="$(on node-b "cat $P/notes.txt")"
  copy_a="$(on node-a "cat $P/notes\ \(conflict\ *\).txt")"
  if [[ "$kept_a" == "edit on b" && "$kept_b" == "edit on b" ]]; then
    pass "the newer edit (node-b's) keeps notes.txt"
  else
    fail "notes.txt holds '$kept_a' on node-a and '$kept_b' on node-b, not node-b's edit"
  fi
  if [[ "$copy_a" == "edit on a" ]]; then
    pass "node-a's edit is in the conflict copy"
  else
    fail "the conflict copy holds '$copy_a', not node-a's edit"
  fi
else
  fail "the conflict did not settle"
  show_diff
  on node-a "ls -la $P" | sed 's/^/   a| /'
  on node-b "ls -la $P" | sed 's/^/   b| /'
fi
wait_for 60 "both in sync" in_sync && pass "the server sees both in sync" \
  || { fail "the server does not see both in sync"; probe sync-status "$PROJECT" | sed 's/^/   /'; }

summary
