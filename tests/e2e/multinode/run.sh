#!/usr/bin/env bash
# Four nodes on one project, each on its own VM: node-a to node-d.
#
#   1. Convergence: all four change different files at the same moment. All
#      end with the same sha256 manifest.
#   2. Concurrent changes to one path, from every node at one wall-clock
#      moment, for E2E_MULTI_ROUNDS rounds:
#        add    all add it, with different content
#        edit   all edit it
#        tie    all edit it and give it the same mtime
#        del    one deletes it, the others edit it
#        gone   all delete it
#      After each round all four must hold the same manifest, the later
#      mtime must hold the path, every other edit must be in a conflict
#      copy, and a delete must never remove an edit.
#   3. Offline catch-up: node-d stops while the others add, edit, delete and
#      rename. It starts and must reach their manifest, no deleted file may
#      return, and the server must say in sync only once it really is.
set -uo pipefail

E2E="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$E2E/../.." && pwd)"
source "$E2E/lib/common.sh"
source "$E2E/lib/beep.sh"

NODES=(node-a node-b node-c node-d)
ROUNDS="${E2E_MULTI_ROUNDS:-8}"
# Expanded on the VM, also inside quotes there.
P="\$HOME/ws/$PROJECT"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# digest <role> : the sha256 of the project's sorted sha256 manifest.
digest(){
  on "$1" "cd $P && find . -type f ! -path '*/.beep-*' -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum | sha256sum | cut -c1-16"
}

# same_on <role...> : whether those nodes hold one manifest.
same_on(){
  local role
  for role in "$@"; do
    digest "$role" > "$WORK/digest-$role" &
  done
  wait
  [[ "$(for role in "$@"; do cat "$WORK/digest-$role"; done | sort -u | wc -l)" == 1 ]] \
    && [[ -s "$WORK/digest-$1" ]]
}
all_same(){ same_on "${NODES[@]}"; }

show_manifests(){
  local role
  for role in "${NODES[@]}"; do
    on "$role" "cd $P && find . -type f ! -path '*/.beep-*' | wc -l" | sed "s/^/   $role files: /"
  done
  probe sync-status "$PROJECT" | sed 's/^/   /'
}

# converged <seconds> <what> : all four hold one manifest and report in sync.
# Returns 1 when the files differ, 2 when they match but the server does not
# say so: the two are different findings.
converged(){
  if ! poll "$1" all_same; then
    fail "$2: the nodes did not converge in $1 s"
    show_manifests
    return 1
  fi
  poll 120 in_sync && return 0
  fail "$2: the files match but the server does not see all four in sync"
  show_manifests
  return 2
}

# status_of <role> : the server's line for a node, without its machine id.
status_of(){
  probe sync-status "$PROJECT" | awk -v id="${MACHINE[$1]}" '$1 == id { $1 = ""; print }'
}

# moment <seconds ahead> : a wall-clock time for every node to act at.
moment(){ awk -v n="$(now)" -v a="$1" 'BEGIN { printf "%d", n + a + 1 }'; }

# until_moment <time> : remote shell that sleeps up to that time.
until_moment(){
  echo "sleep \$(awk -v t=$1 -v n=\$(date +%s.%N) 'BEGIN { d = t - n; print (d > 0 ? d : 0) }')"
}

step "fresh world with four nodes"
fresh_world
turn_on_sync
fd_watch

step "1. all four change different files at one moment"
on node-a "cd $P && mkdir -p conv && for n in 0 1 2 3; do for i in 0 1 2; do echo \"seed \$n \$i\" > conv/s\$n-\$i.txt; done; done" \
  || abort "could not seed"
converged 180 "the seed files" || abort "the seed files did not reach every node"
T="$(moment 4)"
n=0
t0="$(now)"
for role in "${NODES[@]}"; do
  on "$role" "$(until_moment "$T"); cd $P/conv
echo \"edit by $role\" > s$n-0.txt; rm s$n-1.txt; mkdir -p moved-$n; mv s$n-2.txt moved-$n/
mkdir -p own-$n/deep/er; for i in \$(seq 1 200); do echo \"$role \$i\" > own-$n/deep/er/f\$i.txt; done
head -c 20M /dev/urandom > own-$n/big.bin" &
  n=$((n + 1))
done
wait
if converged 300 "changes from every node"; then
  pass "all four hold one manifest after changes from every node"
  measure "multinode: convergence (800 files, 80 MB)" "$(since "$t0")" s max "${E2E_BUDGET_MULTI_CONVERGE-240}"
  odd="$(on node-a "cd $P/conv && ls s?-1.txt s?-2.txt 2> /dev/null; find . -name '*(conflict*'")"
  if [[ -z "$odd" ]]; then pass "deletes and renames held, no conflict copies"; else
    fail "left after different-file changes: $odd"
  fi
fi

fd_mark

step "2. one path changed on every node at one moment, $ROUNDS rounds"
on node-a "cd $P && mkdir -p cases/edit cases/tie cases/del cases/gone cases/add
for r in \$(seq 1 $ROUNDS); do for c in edit tie del gone; do echo \"seed \$r\" > cases/\$c/r\$r.txt; done; done" \
  || abort "could not seed the rounds"
converged 180 "the round seeds" || abort "the round seeds did not reach every node"

# Tally per case.
declare -A PATHS NEWEST COPIES LOST DIVERGED
for c in add edit tie del gone; do
  PATHS[$c]=0 NEWEST[$c]=0 COPIES[$c]=0 LOST[$c]=0 DIVERGED[$c]=0
done
WORST_SPREAD=0

# judge <case> <round> <deleter or ""> : what the round left in that case.
judge(){
  local c="$1" r="$2" deleter="$3" role text at_path copies want_copies=0 lost=0 best=() best_at=-1 mt
  PATHS[$c]=$((PATHS[$c] + 1))
  # Everything for this path: `name<TAB>content`.
  on node-a "cd $P/cases/$c && for f in r$r.txt 'r$r (conflict '*; do [ -f \"\$f\" ] && printf '%s\t%s\n' \"\$f\" \"\$(cat \"\$f\")\"; done; true" > "$WORK/left"
  at_path="$(awk -F'\t' -v f="r$r.txt" '$1 == f { print $2 }' "$WORK/left")"
  copies="$(grep -c '(conflict ' "$WORK/left")"
  if [[ "$c" == gone ]]; then
    if [[ -n "$at_path" || "$copies" != 0 ]]; then
      fail "gone round $r: left $(cat "$WORK/left")"
    fi
    return
  fi
  for role in "${NODES[@]}"; do
    [[ "$role" == "$deleter" ]] && continue
    want_copies=$((want_copies + 1))
    text="$c r$r $role"
    grep -qF "$text" "$WORK/left" || { lost=$((lost + 1)); fail "$c round $r: the edit by $role is gone"; }
    # Newest mtime the writer reported for this file.
    mt="$(awk -v f="$c/r$r.txt" '$1 == "M" && $2 == f { print $3 }' "$WORK/out-$r-$role")"
    # Edits in one millisecond tie, and the hash picks among them.
    if awk -v m="${mt:-0}" -v b="$best_at" 'BEGIN { exit !(m > b) }'; then best=("$text"); best_at="$mt"
    elif [[ "$mt" == "$best_at" ]]; then best+=("$text"); fi
  done
  want_copies=$((want_copies - 1))
  LOST[$c]=$((LOST[$c] + lost))
  if [[ "$copies" == "$want_copies" ]]; then COPIES[$c]=$((COPIES[$c] + 1)); else
    fail "$c round $r: $copies conflict copies, expected $want_copies"
    sed 's/^/   /' "$WORK/left"
  fi
  if [[ "$c" == tie ]]; then
    # One mtime: the hash decides, which only has to be one answer.
    [[ -n "$at_path" ]] && NEWEST[$c]=$((NEWEST[$c] + 1))
  elif printf '%s\n' "${best[@]}" | grep -qxF -- "$at_path"; then
    NEWEST[$c]=$((NEWEST[$c] + 1))
  else
    fail "$c round $r: '$at_path' holds the path, the newest edit was '${best[*]}'"
  fi
}

for ((r = 1; r <= ROUNDS; r++)); do
  T="$(moment 4)"
  deleter="${NODES[$((r % 4))]}"
  for role in "${NODES[@]}"; do
    # Same order on every node, at one moment; `M` lines give the mtimes.
    if [[ "$role" == "$deleter" ]]; then del_op="rm del/r$r.txt"; else del_op="echo 'del r$r $role' > del/r$r.txt"; fi
    on "$role" "$(until_moment "$T"); cd $P/cases; echo S \$(date +%s.%3N)
mkdir -p add; echo 'add r$r $role' > add/r$r.txt; echo 'edit r$r $role' > edit/r$r.txt
echo 'tie r$r $role' > tie/r$r.txt; touch -d @$T tie/r$r.txt
$del_op; rm gone/r$r.txt
for f in add/r$r.txt edit/r$r.txt del/r$r.txt; do [ -f \$f ] && echo M \$f \$(stat -c %.3Y \$f); done; true" > "$WORK/out-$r-$role" &
  done
  wait
  spread="$(cat "$WORK"/out-"$r"-node-* | awk '$1 == "S" { if (!m || $2 < m) m = $2; if ($2 > x) x = $2 } END { printf "%.3f", x - m }')"
  WORST_SPREAD="$(awk -v a="$WORST_SPREAD" -v b="$spread" 'BEGIN { print (b > a ? b : a) }')"
  converged 180 "round $r"
  if [[ "$?" == 1 ]]; then
    for c in add edit tie del gone; do DIVERGED[$c]=$((DIVERGED[$c] + 1)); done
  else
    judge add "$r" ""
    judge edit "$r" ""
    judge tie "$r" ""
    judge del "$r" "$deleter"
    judge gone "$r" ""
  fi
  note "round $r done, start spread ${spread} s"
done
# A result that only looks settled would drift here.
sleep 15
if all_same; then pass "still one manifest 15 s after the last round"; else fail "the nodes drifted after the rounds"; show_manifests; fi
note "the rounds started within $WORST_SPREAD s of each other (by the VMs' clocks)"
printf '   %-6s %8s %10s %12s %12s %9s\n' case paths diverged newest-wins copies-right lost-edits
for c in add edit tie del gone; do
  printf '   %-6s %8s %10s %12s %12s %9s\n' "$c" "${PATHS[$c]}" "${DIVERGED[$c]}" "${NEWEST[$c]}" "${COPIES[$c]}" "${LOST[$c]}"
done

fd_mark

step "3. node-d offline while the others change the project"
on node-a "cd $P && mkdir -p base/keep
for d in 0 1 2 3 4 5 6 7 8 9; do mkdir -p base/d\$d/sub; for i in \$(seq 1 100); do echo \"base \$d \$i\" > base/d\$d/sub/f\$i.txt; done; done
for i in 1 2 3 4 5; do head -c 5M /dev/urandom > base/keep/b\$i.bin; done" || abort "could not make the base tree"
converged 300 "the base tree" || abort "the base tree did not reach every node"
on node-a "cd $P && find . -type f ! -path '*/.beep-*' | LC_ALL=C sort" > "$WORK/before"
stop_node node-d
note "node-d is stopped"
offline_state="$(status_of node-d)"
note "server on node-d while stopped: $offline_state"
on node-a "cd $P/base && for i in \$(seq 1 30); do echo \"edited by a\" > d0/sub/f\$i.txt; done
rm -r d1; for i in \$(seq 1 40); do rm d2/sub/f\$i.txt; done
mkdir -p new-a/x/y/z; for i in \$(seq 1 1000); do echo \"new a \$i\" > new-a/x/y/z/f\$i.txt; done
head -c 100M /dev/urandom > new-a/big.bin" &
on node-b "cd $P/base && for i in \$(seq 1 30); do echo \"edited by b\" > d3/sub/f\$i.txt; done
mv d4 moved-d4; for i in \$(seq 1 40); do mv d5/sub/f\$i.txt d5/renamed-\$i.txt; done
mkdir -p new-b/p/q; for i in \$(seq 1 500); do echo \"new b \$i\" > new-b/p/q/f\$i.txt; done" &
on node-c "cd $P/base && rm -r d6; rm keep/b1.bin; echo 'edited by c' > d7/sub/f1.txt
mkdir -p new-c/m; for i in \$(seq 1 500); do echo \"new c \$i\" > new-c/m/f\$i.txt; done" &
wait
if poll 600 same_on node-a node-b node-c; then
  pass "node-a, node-b and node-c converged without node-d"
else
  fail "the three online nodes did not converge"
  show_manifests
fi
want="$(digest node-a)"
on node-a "cd $P && find . -type f ! -path '*/.beep-*' | LC_ALL=C sort" > "$WORK/after"
LC_ALL=C comm -23 "$WORK/before" "$WORK/after" > "$WORK/deleted"
note "$(wc -l < "$WORK/after") files expected on node-d, $(wc -l < "$WORK/deleted") deleted while it was off"
[[ "$offline_state" == *in_sync* ]] && fail "the server said node-d is in sync while it was stopped: $offline_state"

t0="$(now)"
start_node node-d
# The status is read before the files, so "in_sync" with the wrong files
# means the node said so while files were really missing.
false_sync=0 real_sync="" seen_syncing=0 last=""
while awk -v s="$(since "$t0")" 'BEGIN { exit !(s < 900) }'; do
  state="$(status_of node-d)"
  have="$(digest node-d)"
  count="$(on node-d "cd $P && find . -type f ! -path '*/.beep-*' | wc -l")"
  word="$(awk '{ print $1 }' <<< "$state")"
  if [[ "$word:$count" != "$last" ]]; then
    note "+$(since "$t0") s: $state, $count files"
    last="$word:$count"
  fi
  [[ "$word" == syncing || "$word" == scanning ]] && seen_syncing=1
  if [[ "$state" == *in_sync* && "$state" == *pending=0* ]]; then
    if [[ "$have" == "$want" ]]; then
      real_sync="$(since "$t0")"
      break
    fi
    false_sync=$((false_sync + 1))
    note "FALSE IN SYNC at +$(since "$t0") s: $count files, $state"
  fi
  sleep 1
done
if [[ -n "$real_sync" ]]; then
  pass "node-d holds the others' manifest and the server says in sync, after $real_sync s"
  measure "multinode: offline catch-up" "$real_sync" s max "${E2E_BUDGET_MULTI_CATCHUP-300}"
else
  fail "node-d did not reach a real in sync (have $(digest node-d), want $want)"
  show_manifests
fi
if [[ "$false_sync" == 0 ]]; then
  pass "the server never said in sync while node-d was missing files"
else
  fail "the server said in sync $false_sync time(s) while node-d was missing files"
fi
[[ "$seen_syncing" == 1 ]] && note "the server showed node-d syncing during catch-up"
# Files and status must also agree the other way: right files, status follows.
if poll 60 in_sync; then pass "the server sees all four in sync"; else fail "the server does not see all four in sync"; show_manifests; fi
# A delete that came back would come back after the first rounds settle.
sleep 30
returned=0
for role in "${NODES[@]}"; do
  put "$role" "$WORK/deleted" beep/deleted.txt || abort "could not copy the deleted list"
  back="$(on "$role" "cd $P && while IFS= read -r f; do [ -e \"\$f\" ] && echo \"\$f\"; done < ~/beep/deleted.txt; true")"
  if [[ -n "$back" ]]; then
    returned=1
    fail "deleted files back on $role: $(wc -l <<< "$back"), like $(head -1 <<< "$back")"
  fi
done
[[ "$returned" == 0 ]] && pass "no deleted file came back on any node"
if all_same; then pass "all four still hold one manifest"; else fail "the manifests differ 30 s after catch-up"; show_manifests; fi
copies="$(on node-a "cd $P/base && find . -name '*(conflict*' | wc -l")"
if [[ "$copies" == 0 ]]; then pass "the catch-up made no conflict copies"; else fail "the catch-up made $copies conflict copies"; fi

fd_report
summary
