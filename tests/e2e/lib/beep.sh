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
  for role in server node-a node-b; do
    on "$role" 'mkdir -p ~/beep/bin ~/beep/state ~/beep/logs' || return 1
  done
  ( ship server beep-server && ship server beep-probe \
      && on server "docker pull -q $PG_IMAGE > /dev/null" ) &
  pids+=($!)
  for role in node-a node-b; do
    ship "$role" beep-node &
    pids+=($!)
  done
  for role in "${pids[@]}"; do wait "$role" || failed=1; done
  rm -rf "$STAGE"
  [[ "$failed" == 0 ]] || { echo "deploy failed" >&2; return 1; }
  pass "binaries on all three VMs"
}

# fresh_world : stop everything, wipe all state and the database, then start
# the server and link both nodes to one account. Sets SERVER_ID, NODE_A and
# NODE_B (the nodes' machine ids).
fresh_world(){
  step "fresh server, database and nodes"
  stop_node node-a KILL
  stop_node node-b KILL
  on node-a 'rm -rf ~/beep/state ~/ws ~/beep/logs/*; mkdir -p ~/beep/state ~/ws' || true
  on node-b 'rm -rf ~/beep/state ~/ws ~/beep/logs/*; mkdir -p ~/beep/state ~/ws' || true
  on server "$(stop_pid server INT); docker rm -f beep-pg > /dev/null 2>&1; rm -rf ~/beep/state ~/beep/logs/*; mkdir -p ~/beep/state
docker run -d --name beep-pg -e POSTGRES_USER=beep -e POSTGRES_PASSWORD=beep -e POSTGRES_DB=beep \
  -p 127.0.0.1:55432:5432 $PG_IMAGE > /dev/null
for i in \$(seq 1 60); do docker exec beep-pg pg_isready -U beep > /dev/null 2>&1 && exit 0; sleep 1; done
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
  NODE_A="$(link_node node-a)" || abort "node-a did not link"
  NODE_B="$(link_node node-b)" || abort "node-b did not link"
  note "node-a $NODE_A"
  note "node-b $NODE_B"
  pass "both nodes linked to one account"
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
  for role in server node-a node-b; do
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

# turn_on_sync : the project folder on both nodes, synced, as the app does.
turn_on_sync(){
  on node-a "mkdir -p ~/ws/$PROJECT" && on node-b "mkdir -p ~/ws/$PROJECT"
  local folder_a folder_b
  folder_a="$(on node-a "realpath ~/ws/$PROJECT")"
  folder_b="$(on node-b "realpath ~/ws/$PROJECT")"
  probe sync-on "$PROJECT" "$NODE_A" "$folder_a" > /dev/null || abort "sync-on node-a refused"
  probe sync-on "$PROJECT" "$NODE_B" "$folder_b" > /dev/null || abort "sync-on node-b refused"
  pass "sync on for $PROJECT on both nodes"
}

# in_sync : the server reports both machines in sync with nothing pending.
in_sync(){
  local out; out="$(probe sync-status "$PROJECT")" || return 1
  [[ "$(grep -c ' in_sync files=.* pending=0 ' <<< "$out")" == 2 ]]
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
