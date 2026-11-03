# End-to-end tests

Real `beep-server` and `beep-node` processes on three [Boat](https://boat.dev)
VMs: `server` (beep-server, Postgres in Docker, and `beep-probe` playing the
phone), `node-a` and `node-b`.

    just e2e            # every scenario on one fleet
    just e2e sync       # one scenario
    just e2e all teardown

A run builds static musl binaries (`x86_64-unknown-linux-musl`, needs
`musl-gcc`), makes the VMs (`boat new --no-env --type small`), copies the
binaries over, runs the scenarios, and then always prints the Boat cost and
deletes the VMs, on failure and on Ctrl-C too. On a failure it first copies
every VM's logs to `tests/e2e/.logs/` and prints their tails.

Each scenario starts from nothing: a new database and server, both nodes
linked with setup tokens (`CreateNodeToken`, `beep-node setup`), and project
`e2e` synced at `~/ws/e2e` on both (`SetProjectSync`). The VMs only dial out;
iroh finds its paths through relays and hole punching, so nothing is hosted.

| Scenario | Checks |
| --- | --- |
| `sync` | New small files, a nested tree and a 48 MB random file on node-a reach node-b with the same sha256. Edits, a deleted file and folder, a renamed file and folder follow. A file from node-b reaches node-a. A conflicting edit made while node-b is stopped leaves the newer edit at the path and the other in a `name (conflict xxxxxxxx).ext` copy, on both nodes. |
| `reconnect` | With node-a's egress capped by tc, node-b is stopped partway through fetching a 200 MB file, with SIGTERM and then SIGKILL, and started again. The file arrives whole, no partial folder is left and the server sees both in sync. |

Env:

- `E2E_KEEP=1` keeps the VMs after a run; `E2E_SKIP_BUILD=1` reuses the last build.
- `E2E_BOAT_TYPE` (default `small`), `E2E_TTL_HOURS` (default 2: how long a
  VM lives if teardown never runs).
- `E2E_BIG_MB`, `E2E_RESUME_MB`, `E2E_RESUME_RATE` size the transfers.

A new scenario is a `tests/e2e/<name>/run.sh` that sources `lib/common.sh`
and `lib/beep.sh`, calls `fresh_world`, and ends with `summary`; add its name
to `SCENARIOS` in `e2e.sh`.
