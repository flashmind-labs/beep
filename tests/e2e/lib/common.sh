# Shared helpers for the Beep e2e scenarios. Sourced by tests/e2e/e2e.sh and
# by each scenario's run.sh after it sets E2E (the tests/e2e directory).
# Provides the PASS/FAIL tally, running commands on a role's VM, and waiting.

FLEET="${FLEET:-$E2E/.fleet}"

FAILS=0
pass(){ printf '  \033[32mPASS\033[0m %s\n' "$*"; }
fail(){ printf '  \033[31mFAIL\033[0m %s\n' "$*"; FAILS=$((FAILS+1)); }
step(){ printf '\n\033[1m== %s ==\033[0m\n' "$*"; }
note(){ printf '   %s\n' "$*"; }

# summary : the final tally; exits non-zero if any check failed.
summary(){
  step "summary"
  if [[ "$FAILS" -eq 0 ]]; then
    printf '\033[32mALL CHECKS PASSED\033[0m\n'; exit 0
  else
    printf '\033[31m%d CHECK(S) FAILED\033[0m\n' "$FAILS"; exit 1
  fi
}

# abort <message> : fail and stop the scenario; the dispatcher fetches logs.
abort(){ fail "$*"; echo "aborting"; exit 1; }

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
