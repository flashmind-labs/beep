# Beep on the e2e fleet: build the static binaries, ship them, and start a
# fresh server and two linked nodes for each scenario. Sourced after
# lib/common.sh.
#
# Layout on every VM (the sandbox's own `user`):
#   ~/beep/bin    beep-server and beep-probe (server), beep-node (nodes)
#   ~/beep/state  the server's key and config, the phone's key, a node's --dir
#   ~/beep/logs   server.log, node.log
#   ~/ws          a node's --workspace, the folder it shares
# Postgres runs in Docker on the server VM, bound to 127.0.0.1 only.

TARGET=x86_64-unknown-linux-musl
BIN_DIR="$ROOT/target/$TARGET/release"
PROJECT=e2e
# The nodes a scenario links and syncs; it may set more after sourcing this.
NODES=(node-a node-b)
declare -A MACHINE

# fleet_nodes : every node role the fleet has, used by this scenario or not.
fleet_nodes(){ awk '$2 ~ /^node-/ { print $2 }' "$FLEET"; }

# What a node runs with: its state, its workspace and its logging.
NODE_ENV="RUST_LOG=${E2E_NODE_LOG:-info,beep_node=debug,iroh=warn,tracing::span=warn}"
NODE_CMD='~/beep/bin/beep-node --dir ~/beep/state --workspace ~/ws'
PG_IMAGE=pgvector/pgvector:pg17
DB_URL=postgres://beep:beep@127.0.0.1:55432/beep

# build : the static musl binaries, once per run. E2E_SKIP_BUILD=1 reuses them.
build(){
  if [[ "${E2E_SKIP_BUILD:-0}" == 1 ]]; then
    note "E2E_SKIP_BUILD=1: using the binaries in $BIN_DIR"
    return 0
  fi
  step "build static beep-server, beep-probe and beep-node ($TARGET)"
  ( cd "$ROOT" && CC_x86_64_unknown_linux_musl="${CC_x86_64_unknown_linux_musl:-musl-gcc}" \
      cargo build --release --target "$TARGET" -p beep-server --bin beep-server --bin beep-probe \
      -p beep-node --bin beep-node )
}

# ship <role> <binary> : a stripped, gzipped copy to ~/beep/bin on a VM.
ship(){
  local role="$1" bin="$2"
  put "$role" "$STAGE/$bin.gz" "beep/bin/$bin.gz" \
    && on "$role" "gunzip -f ~/beep/bin/$bin.gz && chmod +x ~/beep/bin/$bin"
}

# deploy : copy the binaries to the fleet and pull Postgres on the server.
deploy(){
  step "deploy binaries"
  local f
  STAGE="$(mktemp -d)"
  for f in beep-server beep-probe beep-node; do
    [[ -x "$BIN_DIR/$f" ]] || { echo "missing $BIN_DIR/$f: build first" >&2; return 1; }
    strip -o "$STAGE/$f" "$BIN_DIR/$f" && gzip -1 "$STAGE/$f" || return 1
  done
  local pids=() role failed=0
  for role in server $(fleet_nodes); do
    on "$role" 'mkdir -p ~/beep/bin ~/beep/state ~/beep/logs' || return 1
  done
  ( ship server beep-server && ship server beep-probe \
      && on server "docker pull -q $PG_IMAGE > /dev/null" ) &
  pids+=($!)
  for role in $(fleet_nodes); do
    ship "$role" beep-node &
    pids+=($!)
  done
  for role in "${pids[@]}"; do wait "$role" || failed=1; done
  rm -rf "$STAGE"
  [[ "$failed" == 0 ]] || { echo "deploy failed" >&2; return 1; }
  pass "binaries on all three VMs"
}

# fresh_world : stop everything, wipe all state and the database, then start
# the server and link $NODES to one account. Sets SERVER_ID and MACHINE (each
# node's machine id by role).
fresh_world(){
  step "fresh server, database and nodes"
  local role
  for role in $(fleet_nodes); do
    stop_node "$role" KILL
    on "$role" 'rm -rf ~/beep/state ~/ws ~/beep/logs/*; mkdir -p ~/beep/state ~/ws' || true
  done
  on server "$(stop_pid server INT); docker rm -f beep-pg > /dev/null 2>&1; rm -rf ~/beep/state ~/beep/logs/*; mkdir -p ~/beep/state
docker run -d --name beep-pg -e POSTGRES_USER=beep -e POSTGRES_PASSWORD=beep -e POSTGRES_DB=beep \
  -p 127.0.0.1:55432:5432 $PG_IMAGE > /dev/null
for i in \$(seq 1 60); do docker logs beep-pg 2>&1 | grep -q 'init process complete' && docker exec beep-pg pg_isready -U beep > /dev/null 2>&1 && exit 0; sleep 1; done
exit 1" || abort "Postgres did not start"
  # No model is ever called: the key only has to be present.
  on server "cat > ~/beep/state/config.toml <<EOF
secret_key = \"\$HOME/beep/state/server-key\"
database_url = \"$DB_URL\"
model = \"openrouter:openai/gpt-6-luna\"
provider_key = \"e2e-no-model\"
EOF"
  # Every descriptor redirected, or the ssh session waits on the server.
  on server '(cd ~/beep/state && RUST_LOG=info,beep_server=debug,iroh=warn,tracing::span=warn exec setsid ~/beep/bin/beep-server ~/beep/state/config.toml) >> ~/beep/logs/server.log 2>&1 < /dev/null & echo $! > ~/beep/server.pid'
  server_id_seen(){ on server "grep -o 'node_id=[0-9a-f]*' ~/beep/logs/server.log | head -1 | grep -q ." ; }
  wait_for 90 "the server bound its endpoint" server_id_seen || abort "the server did not start"
  SERVER_ID="$(on server "grep -o 'node_id=[0-9a-f]*' ~/beep/logs/server.log | head -1 | cut -d= -f2")"
  note "server $SERVER_ID"
  pass "server up"
  for role in "${NODES[@]}"; do
    MACHINE[$role]="$(link_node "$role")" || abort "$role did not link"
    note "$role ${MACHINE[$role]}"
  done
  pass "${#NODES[@]} nodes linked to one account"
}

# probe <args...> : beep-probe on the server VM, signed in as the phone.
probe(){
  on server "BEEP_IDENTITY=~/beep/state/phone-key ~/beep/bin/beep-probe $SERVER_ID $*"
}

# link_node <role> : make a setup token, link the role's node with it, start
# it, and echo its machine id.
link_node(){
  local role="$1" token id
  token="$(probe token "$role" | tail -1)"
  [[ "$token" == bk1-* ]] || { echo "no setup token: $token" >&2; return 1; }
  on "$role" "$NODE_ENV $NODE_CMD setup $token --no-run --name $role >> ~/beep/logs/node.log 2>&1" || return 1
  start_node "$role"
  id="$(probe token-machine "$token" | tail -1)"
  [[ -n "$id" ]] || { echo "the token was not claimed" >&2; return 1; }
  echo "$id"
}

# fd_watch : sample every node's open fds and inotify use until fd_report.
declare -A FD_MARKS
fd_watch(){
  local role
  for role in "${NODES[@]}"; do
    put "$role" "$E2E/lib/fd-sample.sh" beep/fd-sample.sh || abort "could not copy fd-sample.sh"
    on "$role" "rm -f ~/beep/fds.log; (exec setsid bash ~/beep/fd-sample.sh ~/beep/fds.log) > /dev/null 2>&1 < /dev/null & echo \$! > ~/beep/fd-sample.pid"
  done
}

# fd_now <role> : the node's open fds in the last sample.
fd_now(){ on "$1" "tail -1 ~/beep/fds.log" | awk '{ print $2 }'; }

# fd_mark : note each node's fds now that sync has settled.
fd_mark(){
  local role
  for role in "${NODES[@]}"; do FD_MARKS[$role]+=" $(fd_now "$role")"; done
}

# fd_report : peaks against the budgets, and no growth: not from the first
# settled point to the last, and not over a final idle minute.
fd_report(){
  local role peak instances watches first rest last now growth="${E2E_FD_GROWTH:-16}"
  step "open fds and inotify"
  fd_mark
  sleep "${E2E_FD_IDLE:-60}"
  for role in "${NODES[@]}"; do
    now="$(fd_now "$role")"
    read -r first rest <<< "${FD_MARKS[$role]}"
    last="${FD_MARKS[$role]##* }"
    note "$role settled fds:${FD_MARKS[$role]}, $now after the idle wait"
    (( last > first + growth )) && fail "$role fds grew from $first to $last between settled points"
    (( now > last + growth )) && fail "$role fds grew from $last to $now while idle"
    read -r peak instances watches <<< "$(on "$role" "kill \$(cat ~/beep/fd-sample.pid); awk '{ if (\$2 > f) f = \$2; if (\$3 > i) i = \$3; if (\$4 > w) w = \$4 } END { print f + 0, i + 0, w + 0 }' ~/beep/fds.log")"
    measure "$role: peak open fds" "$peak" fds max "${E2E_BUDGET_FDS-128}"
    measure "$role: inotify instances" "$instances" "" max 1
    measure "$role: peak inotify watches" "$watches" "" max ""
  done
}

# start_node <role> / stop_node <role> [signal] : the node process.
start_node(){
  on "$1" "($NODE_ENV exec setsid $NODE_CMD run) >> ~/beep/logs/node.log 2>&1 < /dev/null & echo \$! > ~/beep/node.pid"
}
stop_node(){
  on "$1" "$(stop_pid node "${2:-TERM}")"
}

# stop_pid <name> <signal> : the remote shell that stops the process whose
# pid start_node or fresh_world saved in ~/beep/<name>.pid, waiting up to
# 10 s before SIGKILL. Only that pid: nothing is killed by name.
stop_pid(){
  echo "f=~/beep/$1.pid; if [ -s \$f ]; then p=\$(cat \$f); kill -$2 \$p 2> /dev/null
for i in \$(seq 1 20); do kill -0 \$p 2> /dev/null || break; sleep 0.5; done
kill -KILL \$p 2> /dev/null; rm -f \$f; fi; true"
}

# fetch_logs : copy every VM's logs to tests/e2e/.logs and print their tails.
fetch_logs(){
  [[ -f "$FLEET" ]] || return 0
  step "logs (full copies in $E2E/.logs)"
  local role f
  rm -rf "$E2E/.logs"
  for role in server $(fleet_nodes); do
    mkdir -p "$E2E/.logs/$role"
    for f in server.log node.log; do
      get "$role" "beep/logs/$f" "$E2E/.logs/$role/$f" 2> /dev/null || continue
      echo "--- $role $f (last ${E2E_LOG_LINES:-60} lines) ---"
      tail -n "${E2E_LOG_LINES:-60}" "$E2E/.logs/$role/$f"
    done
    if [[ "$role" != server ]]; then
      echo "--- $role ~/ws ---"
      on "$role" 'cd ~/ws && find . -maxdepth 4 | sort | head -50' 2> /dev/null || true
    fi
  done
}

# listing <role> [dir] : `sha256 path` of every file in the project folder,
# sync's own files left out.
listing(){
  on "$1" "cd ~/ws/$PROJECT${2:+/$2} && find . -type f ! -name '.beep-*' -print0 | sort -z | xargs -0 -r sha256sum"
}

# same_files : whether node-b holds exactly node-a's files.
same_files(){
  local a b
  a="$(listing node-a)" || return 1
  b="$(listing node-b)" || return 1
  [[ -n "$a" && "$a" == "$b" ]]
}

# show_diff : what differs between the two nodes' folders.
show_diff(){
  diff <(listing node-a) <(listing node-b) | head -40 | sed 's/^/   /'
}

# turn_on_sync : the project folder on every node, synced, as the app does.
turn_on_sync(){
  local role folder
  for role in "${NODES[@]}"; do
    folder="$(on "$role" "mkdir -p ~/ws/$PROJECT && realpath ~/ws/$PROJECT")"
    probe sync-on "$PROJECT" "${MACHINE[$role]}" "$folder" > /dev/null || abort "sync-on $role refused"
  done
  pass "sync on for $PROJECT on ${#NODES[@]} nodes"
}

# in_sync : the server reports every machine in sync with nothing pending.
in_sync(){
  local out; out="$(probe sync-status "$PROJECT")" || return 1
  [[ "$(grep -c ' in_sync files=.* pending=0 ' <<< "$out")" == "${#NODES[@]}" ]]
}

# synced <seconds> <what> : wait until node-b matches node-a file for file
# and the server sees both in sync; PASS or FAIL with the difference.
synced(){
  local limit="$1" what="$2"
  if wait_for "$limit" "$what" same_files && wait_for 60 "both in sync" in_sync; then
    pass "$what"
  else
    fail "$what"
    show_diff
    probe sync-status "$PROJECT" | sed 's/^/   /'
  fi
}

# drop_file <role> <name> <mb> : a random file of <mb> MB written beside the
# workspace, then moved into the project folder in one rename, so sync only
# ever sees it whole.
drop_file(){
  on "$1" "head -c ${3}M /dev/urandom > ~/beep/drop.tmp && mv ~/beep/drop.tmp ~/ws/$PROJECT/$2"
}

# same_file <name> : whether the file has the same sha256 on both nodes.
same_file(){
  local a b
  a="$(on node-a "sha256sum < ~/ws/$PROJECT/$1")" || return 1
  b="$(on node-b "sha256sum < ~/ws/$PROJECT/$1")" || return 1
  [[ -n "$a" && "$a" == "$b" ]]
}

# file_rate <from> <to> <name> <mb> <what> <min MB/s> : time a new <mb> MB
# file on <from> until it is at <to>, where sync puts a file only once it is
# whole. Checks its sha256 and records MB/s end to end: the sender's scan
# and hashing, the transfer, and the receiver's copy into the folder.
file_rate(){
  local from="$1" to="$2" name="$3" mb="$4" what="$5" floor="$6" t0 secs
  drop_file "$from" "$name" "$mb" || abort "could not write $name on $from"
  t0="$(now)"
  if ! poll 600 on "$to" "test -f ~/ws/$PROJECT/$name"; then
    fail "$name did not reach $to"
    return
  fi
  secs="$(since "$t0")"
  if same_file "$name"; then pass "$name ($mb MB) on $to in $secs s"; else fail "$name differs on $to"; fi
  measure "$what" "$(awk -v m="$mb" -v s="$secs" 'BEGIN { printf "%.1f", m / s }')" MB/s min "$floor"
}

# The interfaces holding a VM's IPv4 and IPv6 default routes: iroh may send
# over either, or through a relay over either. Expanded on the VM.
UPLINKS="{ ip -4 route show default; ip -6 route show default; } | awk '{for(i=1;i<NF;i++) if(\$i==\"dev\") print \$(i+1)}' | sort -u"

# cap <role> <rate> / uncap <role> : limit a VM's egress on every uplink.
cap(){
  on "$1" "for dev in \$($UPLINKS); do sudo tc qdisc replace dev \$dev root tbf rate $2 burst 256kb latency 400ms || exit 1; done"
}
uncap(){
  on "$1" "for dev in \$($UPLINKS); do sudo tc qdisc del dev \$dev root 2> /dev/null; done; true"
}

# rx_bytes <role> : bytes the VM has received on its uplinks since boot.
rx_bytes(){
  on "$1" "for dev in \$($UPLINKS); do cat /sys/class/net/\$dev/statistics/rx_bytes; done | awk '{ s += \$1 } END { printf \"%d\\n\", s }'"
}

# fetching_mb <role> : MB of the blob the node received last. A fetch lands
# in the node's blob store first and is copied into the folder once whole.
fetching_mb(){
  on "$1" "f=\$(ls -t ~/beep/state/blobs/data/*.data 2> /dev/null | head -1)
if [ -n \"\$f\" ]; then du -m \"\$f\" | cut -f1; else echo 0; fi"
}

# leftovers <role> : any partial folder in the workspace and any journal
# entry for one; prints nothing when clean.
leftovers(){
  on "$1" "find ~/ws -name '.beep-*.partial'; find ~/beep/state/partial -mindepth 1 2> /dev/null; true"
}
