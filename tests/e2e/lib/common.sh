# Shared helpers for the Beep e2e scenarios. Sourced by tests/e2e/e2e.sh and
# by each scenario's run.sh after it sets E2E (the tests/e2e directory).
# Provides the PASS/FAIL tally, running commands on a role's VM, and waiting.

FLEET="${FLEET:-$E2E/.fleet}"

FAILS=0
pass(){ printf '  \033[32mPASS\033[0m %s\n' "$*"; }
fail(){ printf '  \033[31mFAIL\033[0m %s\n' "$*"; FAILS=$((FAILS+1)); }
step(){ printf '\n\033[1m== %s ==\033[0m\n' "$*"; }
note(){ printf '   %s\n' "$*"; }

# Measurements: `measure` records one and checks it against its budget;
# summary prints them all as a table.
MEASURED=()

# now : seconds since the epoch, to the millisecond, on this machine. Every
# time is taken here, so the VMs' clocks never matter. A poll is one ssh
# round trip (about 1 s), so times are good to a second or so.
now(){ date +%s.%3N; }

# since <start> : seconds from <start> (a `now`) to now, one decimal.
since(){ awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.1f", b - a }'; }

# measure <what> <value> <unit> <max|min> <budget> : record a measurement
# and FAIL when it is over a max budget or under a min one. An empty
# budget only records it.
measure(){
  local what="$1" value="$2" unit="$3" kind="$4" budget="$5" verdict=ok
  if [[ -n "$budget" ]]; then
    if awk -v v="$value" -v b="$budget" -v k="$kind" \
        'BEGIN { exit !((k == "max" && v > b) || (k == "min" && v < b)) }'; then
      verdict=OVER
      [[ "$kind" == min ]] && verdict=UNDER
      fail "$what: $value $unit, budget $kind $budget"
    fi
  fi
  MEASURED+=("$what|$value $unit|${budget:+$kind $budget}|$verdict")
}

# timings : the measurements as a table.
timings(){
  [[ ${#MEASURED[@]} -gt 0 ]] || return 0
  printf '   %-46s %12s %12s  %s\n' measurement value budget ""
  local row what value budget verdict
  for row in "${MEASURED[@]}"; do
    IFS='|' read -r what value budget verdict <<< "$row"
    printf '   %-46s %12s %12s  %s\n' "$what" "$value" "${budget:--}" "$verdict"
  done
}

# summary : the measurements and the final tally; exits non-zero if any
# check failed.
summary(){
  step "summary"
  timings
  if [[ "$FAILS" -eq 0 ]]; then
    printf '\033[32mALL CHECKS PASSED\033[0m\n'; exit 0
  else
    printf '\033[31m%d CHECK(S) FAILED\033[0m\n' "$FAILS"; exit 1
  fi
}

# abort <message> : fail and stop the scenario; the dispatcher fetches logs.
abort(){ fail "$*"; timings; echo "aborting"; exit 1; }

# vm_id <role> : the sandbox id of server, node-a or node-b.
vm_id(){
  local id role
  [[ -f "$FLEET" ]] || { echo "no $FLEET: provision first" >&2; return 1; }
  while read -r id role; do
    [[ "$role" == "$1" ]] && { echo "$id"; return 0; }
  done < "$FLEET"
  echo "no $1 in $FLEET" >&2
  return 1
}

# on <role> <command> : run a shell command on a role's VM as its `user`.
# Never reads stdin, so it is safe inside a `while read` loop.
on(){
  local role="$1"; shift
  local id; id="$(vm_id "$role")" || return 1
  boat ssh "$id" -- "$*" < /dev/null
}

# put <role> <local file> <remote path> : copy a file to a role's VM.
put(){
  local id; id="$(vm_id "$1")" || return 1
  boat scp "$2" "$id:$3" > /dev/null
}

# get <role> <remote path> <local file> : copy a file from a role's VM.
get(){
  local id; id="$(vm_id "$1")" || return 1
  boat scp "$id:$2" "$3" > /dev/null
}

# poll <seconds> <command...> : run the command back to back until it
# succeeds, for timing: each `on` is an ssh round trip, which paces it.
# Returns 1 after <seconds>.
poll(){
  local limit="$1"; shift
  local started=$SECONDS
  until "$@" > /dev/null 2>&1; do
    (( SECONDS - started < limit )) || return 1
    sleep 0.2
  done
}

# wait_for <seconds> <what> <command...> : run the command every 2 s until it
# succeeds. Returns 1 after <seconds>.
wait_for(){
  local limit="$1" what="$2"; shift 2
  local started=$SECONDS
  while (( SECONDS - started < limit )); do
    if "$@" > /dev/null 2>&1; then
      note "$what after $((SECONDS - started)) s"
      return 0
    fi
    sleep 2
  done
  note "gave up waiting for $what after $limit s"
  return 1
}
