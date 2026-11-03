# Boat sandbox fleet for the e2e scenarios (https://boat.dev). Sourced by
# tests/e2e/e2e.sh after lib/common.sh.
#
# One fleet serves every scenario: a `server` VM (beep-server, Postgres in
# Docker, the beep-probe that plays the phone) and two node VMs, `node-a` and
# `node-b`. $FLEET records `id role` per line, written as each VM is made, so
# teardown finds every billed VM even after a half-finished provision.
#
# Every VM is made with --no-env: none of the account's stored secrets reach
# it. Nothing is hosted or forwarded: iroh only dials out (relays and hole
# punching), so the VMs keep their firewall as Boat sets it.
#
# Env: E2E_BOAT_TYPE (default small), E2E_TTL_HOURS (how long a VM may live
# if teardown never runs, default 2).

export BOAT_NO_UPDATE=1

ROLES=(server node-a node-b)

# provision : make the fleet, in parallel, unless $FLEET already exists.
provision(){
  if [[ -f "$FLEET" ]]; then
    note "reusing the fleet in $FLEET"
    cat "$FLEET"
    return 0
  fi
  command -v boat > /dev/null || { echo "boat not found (https://boat.dev)" >&2; return 1; }
  command -v jq > /dev/null || { echo "jq not found" >&2; return 1; }
  step "provision: ${ROLES[*]} (boat ${E2E_BOAT_TYPE:-small})"
  : > "$FLEET"
  local role pids=() failed=0
  for role in "${ROLES[@]}"; do
    (
      local id
      id="$(boat new --no-env --type "${E2E_BOAT_TYPE:-small}" --name "beep-e2e-$role" \
              --ttl "$(( ${E2E_TTL_HOURS:-2} * 3600 ))" --json \
            | jq -r 'select(.event == "ready") | .id' | tail -1)"
      [[ -n "$id" && "$id" != null ]] || { echo "   create failed for $role" >&2; exit 1; }
      # One line per write, so the parallel appends do not interleave.
      echo "$id $role" >> "$FLEET"
      note "$role: $id"
    ) &
    pids+=($!)
  done
  for role in "${pids[@]}"; do wait "$role" || failed=1; done
  local made; made="$(wc -l < "$FLEET")"
  if [[ "$failed" != 0 || "$made" != "${#ROLES[@]}" ]]; then
    echo "provisioning failed; $made VM(s) recorded in $FLEET" >&2
    return 1
  fi
}

# cost_report : what each VM in $FLEET has cost so far, and the total.
cost_report(){
  [[ -f "$FLEET" ]] || return 0
  step "boat cost"
  local id role usage cost total=0
  while read -r id role; do
    [[ -n "$id" ]] || continue
    usage="$(boat usage "$id" --json 2> /dev/null < /dev/null || true)"
    cost="$(jq -r '.dollars // empty' <<< "$usage" 2> /dev/null)"
    if [[ -n "$cost" ]]; then
      total="$(awk -v a="$total" -v b="$cost" 'BEGIN { printf "%.5f", a + b }')"
      note "$role ($id): \$$cost for $(jq -r .seconds <<< "$usage") s billed"
    else
      note "$role ($id): $(tr -d '\n' <<< "$usage" | cut -c1-200)"
    fi
  done < "$FLEET"
  note "total: \$$total"
}

# teardown : delete every VM in $FLEET. Keeps the file if any delete fails,
# as it is then the only record of a VM still billed.
teardown(){
  [[ -f "$FLEET" ]] || { note "no $FLEET, nothing to tear down"; return 0; }
  step "teardown"
  local id role failed=0
  while read -r id role; do
    [[ -n "$id" ]] || continue
    note "deleting $role ($id)"
    boat delete "$id" --yes > /dev/null < /dev/null || { note "delete failed for $id, check 'boat list'"; failed=1; }
  done < "$FLEET"
  if [[ "$failed" == 0 ]]; then
    rm -f "$FLEET"
  else
    echo "left $FLEET in place: some VMs could not be deleted" >&2
    return 1
  fi
}
