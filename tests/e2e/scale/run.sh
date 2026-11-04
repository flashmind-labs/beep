#!/usr/bin/env bash
# Sync at scale: tens of thousands of files, a deep tree, a whole-tree
# delete, and how long each takes.
#
# Topology as in the sync scenario: server, node-a and node-b, one project
# synced on both nodes.
#   1. node-a makes tree-a: 20000 small files in a tree 8 folders deep and
#      300 files of 1 MB. node-b must end up with the same files (sha256 of
#      every file). Times: the first file on node-b, and all of them.
#   2. node-a deletes tree-a. node-b must end up with none of it, folders
#      included, and no empty folder anywhere in the project.
#   3. node-b makes tree-b, the same shape, which must reach node-a.
#   4. a large file from node-a to node-b, for MB/s.
# Every time is checked against a budget (see the README).
set -uo pipefail

E2E="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$E2E/../.." && pwd)"
source "$E2E/lib/common.sh"
source "$E2E/lib/beep.sh"

SMALL="${E2E_SCALE_FILES:-20000}"
MB_FILES="${E2E_SCALE_MB_FILES:-300}"
BIG_MB="${E2E_SCALE_BIG_MB:-256}"
WANT=$((SMALL + MB_FILES))
LIMIT=1800

# tree_state <role> <dir> : "<files> <digest>"; the digest, of every file's
# sha256, only once the count is $WANT, and "-" before.
tree_state(){
  on "$1" "cd ~/ws/$PROJECT && n=\$(find $2 -type f 2> /dev/null | wc -l) && d=- && if [ \"\$n\" -eq $WANT ]; then d=\$(cd $2 && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum | sha256sum | cut -c1-16); fi && echo \"\$n \$d\""
}

# left <role> <dir> : "<files> <folders>" still under the folder.
left(){
  on "$1" "cd ~/ws/$PROJECT && echo \$(find $2 -type f 2> /dev/null | wc -l) \$(find $2 -type d 2> /dev/null | wc -l)"
}

# empty_folders <role> : every empty folder in the project.
empty_folders(){ on "$1" "find ~/ws/$PROJECT -mindepth 1 -type d -empty"; }

# grow <from> <to> <dir> <label> <first budget> <full budget> : make the
# tree on <from> while timing it onto <to>, from the moment it starts.
grow(){
  local from="$1" to="$2" dir="$3" label="$4" first_budget="$5" full_budget="$6"
  local t0 gen first="" want_digest="" state n d shown=$SECONDS
  t0="$(now)"
  on "$from" "bash ~/beep/make-tree.sh ~/ws/$PROJECT/$dir $SMALL $MB_FILES" &
  gen=$!
  while :; do
    state="$(tree_state "$to" "$dir")" || state="0 -"
    read -r n d <<< "$state"
    if [[ -z "$first" ]] && (( n > 0 )); then
      first="$(since "$t0")"
      note "first file on $to after $first s"
    fi
    if [[ -z "$want_digest" ]] && ! kill -0 "$gen" 2> /dev/null; then
      wait "$gen" || abort "could not make $dir on $from"
      note "$dir made on $from after $(since "$t0") s"
      read -r n want_digest <<< "$(tree_state "$from" "$dir")"
      [[ "$n" == "$WANT" ]] || abort "$from holds $n files in $dir, not $WANT"
    fi
    [[ -n "$want_digest" && "$d" == "$want_digest" ]] && break
    if (( SECONDS - shown >= 30 )); then
      note "$to: $n of $WANT files"
      shown=$SECONDS
    fi
    if awk -v s="$(since "$t0")" -v l="$LIMIT" 'BEGIN { exit !(s > l) }'; then
      fail "$to holds $n of $WANT files of $dir after $LIMIT s"
      [[ -z "$want_digest" ]] && wait "$gen"
      probe sync-status "$PROJECT" | sed 's/^/   /'
      return 1
    fi
    sleep 1
  done
  local full; full="$(since "$t0")"
  pass "$to holds all $WANT files of $dir, same sha256, after $full s"
  measure "$label: first file" "$first" s max "$first_budget"
  measure "$label: all files" "$full" s max "$full_budget"
  wait_for 120 "both in sync" in_sync && pass "the server sees both in sync" \
    || { fail "the server does not see both in sync"; probe sync-status "$PROJECT" | sed 's/^/   /'; }
}

fresh_world
turn_on_sync
fd_watch
put node-a "$E2E/scale/make-tree.sh" beep/make-tree.sh || abort "could not copy make-tree.sh"
put node-b "$E2E/scale/make-tree.sh" beep/make-tree.sh || abort "could not copy make-tree.sh"

step "1. $SMALL small files 8 folders deep and $MB_FILES files of 1 MB, node-a to node-b"
grow node-a node-b tree-a "scale a->b" "${E2E_BUDGET_SCALE_FIRST-30}" "${E2E_BUDGET_SCALE_ALL-240}" \
  || abort "tree-a did not reach node-b"
fd_mark

step "2. node-a deletes the whole tree"
t0="$(now)"
on node-a "rm -rf ~/ws/$PROJECT/tree-a" || abort "could not delete on node-a"
first="" files_gone_at=""
while :; do
  read -r files folders <<< "$(left node-b tree-a || echo "? ?")"
  if [[ -z "$first" && "$files" =~ ^[0-9]+$ ]] && (( files < WANT )); then
    first="$(since "$t0")"
    note "first delete on node-b after $first s"
  fi
  [[ "$files" == 0 && "$folders" == 0 ]] && break
  if [[ "$files" == 0 && -z "$files_gone_at" ]]; then files_gone_at=$SECONDS; fi
  if [[ -n "$files_gone_at" ]] && (( SECONDS - files_gone_at > 60 )); then
    break
  fi
  if awk -v s="$(since "$t0")" -v l="$LIMIT" 'BEGIN { exit !(s > l) }'; then break; fi
  sleep 1
done
gone="$(since "$t0")"
if [[ "$files" == 0 && "$folders" == 0 ]]; then
  pass "tree-a is gone from node-b, folders and all, after $gone s"
  measure "scale delete: first file gone" "$first" s max "${E2E_BUDGET_SCALE_DELETE_FIRST-20}"
  measure "scale delete: all gone" "$gone" s max "${E2E_BUDGET_SCALE_DELETE-30}"
else
  fail "node-b still holds $files files and $folders folders of tree-a"
  on node-b "cd ~/ws/$PROJECT && find tree-a | head -20" | sed 's/^/   /'
fi
for role in node-a node-b; do
  empty="$(empty_folders "$role")"
  if [[ -z "$empty" ]]; then pass "no empty folder on $role"; else
    fail "empty folders on $role: $(wc -l <<< "$empty")"
    head -10 <<< "$empty" | sed 's/^/   /'
  fi
done
wait_for 120 "both in sync" in_sync && pass "the server sees both in sync" \
  || { fail "the server does not see both in sync"; probe sync-status "$PROJECT" | sed 's/^/   /'; }

fd_mark

step "3. the same tree made on node-b reaches node-a"
grow node-b node-a tree-b "scale b->a" "${E2E_BUDGET_SCALE_FIRST-30}" "${E2E_BUDGET_SCALE_ALL-240}"

fd_mark

step "4. a $BIG_MB MB file, node-a to node-b"
file_rate node-a node-b big.bin "$BIG_MB" "scale: $BIG_MB MB file, MB/s" "${E2E_BUDGET_SCALE_MBPS-5}"

fd_report
summary
