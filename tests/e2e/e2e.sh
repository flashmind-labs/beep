#!/usr/bin/env bash
# End-to-end tests: real beep-server and beep-node processes on Boat VMs.
#
#   tests/e2e/e2e.sh [scenario|all] [action]
#
# Scenarios (tests/e2e/<name>/run.sh):
#   sync       two nodes keep a project folder in step over iroh-blobs: new
#              files, a large file, a nested tree, edits, deletes, renames
#              and a conflicting edit
#   reconnect  a node stopped in the middle of a large sync is started
#              again, and the sync resumes and completes
#   all        (default) every scenario, one after another on one fleet
#
# Actions:
#   run        (default) build, provision, deploy, run, then always fetch
#              logs on failure, print the cost and tear down, also on Ctrl-C
#   provision  make the fleet only (tests/e2e/.fleet)
#   teardown   delete the fleet
#
# Env: E2E_KEEP=1 keeps the fleet after a run (tear it down yourself);
# E2E_SKIP_BUILD=1 reuses the last build; E2E_BOAT_TYPE, E2E_TTL_HOURS (see
# lib/boat.sh).
set -uo pipefail

E2E="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$E2E/../.." && pwd)"
export E2E ROOT
# shellcheck source=lib/common.sh
source "$E2E/lib/common.sh"
# shellcheck source=lib/boat.sh
source "$E2E/lib/boat.sh"
# shellcheck source=lib/beep.sh
source "$E2E/lib/beep.sh"

SCENARIOS=(sync reconnect)
scenario="${1:-all}"
action="${2:-run}"

case "$scenario" in
  all) chosen=("${SCENARIOS[@]}") ;;
  -h|--help|help) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *)
    [[ -f "$E2E/$scenario/run.sh" ]] || { echo "unknown scenario: $scenario" >&2; exit 2; }
    chosen=("$scenario") ;;
esac

case "$action" in
  provision) provision; exit $? ;;
  teardown) cost_report; teardown; exit $? ;;
  run) ;;
  *) echo "unknown action: $action" >&2; exit 2 ;;
esac

STARTED=$SECONDS
status=0
finished=0

# Runs once, on success, failure or Ctrl-C: logs if anything failed, the
# cost, then teardown. Every VM in .fleet is billed until this runs.
finish(){
  [[ "$finished" == 1 ]] && return
  finished=1
  trap '' INT TERM
  if [[ "$status" != 0 ]]; then fetch_logs; fi
  cost_report
  if [[ "${E2E_KEEP:-0}" == 1 ]]; then
    note "E2E_KEEP=1: the fleet stays up; remove it with: $0 all teardown"
  else
    teardown || status=1
  fi
  step "done in $((SECONDS - STARTED)) s"
  for line in "${results[@]}"; do note "$line"; done
  exit "$status"
}
results=()
trap 'status=130; finish' INT TERM
trap 'finish' EXIT

build || { status=1; exit 1; }
provision || { status=1; exit 1; }
deploy || { status=1; exit 1; }

for name in "${chosen[@]}"; do
  begun=$SECONDS
  # Each scenario runs in its own shell, so its `exit` ends only it.
  bash "$E2E/$name/run.sh"
  rc=$?
  results+=("$name: $([[ $rc == 0 ]] && echo passed || echo FAILED) in $((SECONDS - begun)) s")
  # Stop at the first failure: the next scenario would wipe its logs.
  if [[ $rc != 0 ]]; then status=1; break; fi
done
