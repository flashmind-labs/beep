# End-to-end tests

Real `beep-server` and `beep-node` processes on three [Boat](https://boat.dev)
VMs: `server` (beep-server, Postgres in Docker, and `beep-probe` playing the
phone), `node-a` and `node-b`. `multinode` adds `node-c` and `node-d`, two more
VMs that exist only when that scenario is chosen.

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
| `interrupt` | With node-a's egress capped by tc, a 500 MB file is moving to node-b when node-b is restarted, then node-a is restarted, then node-b's network is cut with iptables DROP for 30 s (ssh still let through) and restored. Each time node-b receives about what was left after the interruption, not the whole file again (bytes on its uplinks), the file arrives with node-a's sha256, no partial folder or journal entry is left on either node, and the server sees both in sync. |
| `scale` | node-a makes 20000 small files in a tree 8 folders deep plus 300 files of 1 MB; node-b ends up with the same sorted sha256 manifest. node-a deletes the tree; node-b keeps none of it and no empty folder. The same tree made on node-b reaches node-a. A 256 MB file times MB/s. |
| `powercut` | node-b reboots without a flush (`/proc/sysrq-trigger`) partway through taking in a tree from node-a, three times. Every file at its path after the reboot is whole; node-b then reaches node-a's manifest, node-a's does not change, and there are no conflict copies. |
| `multinode` | Four nodes. Changes from all of them at one moment converge. One path changed on every node at one moment, over rounds (all add, all edit, all edit with one mtime, one deletes while the others edit, all delete): every node picks one winner, the newest edit wins, losing edits stay as conflict copies, a delete never beats an edit. node-d stops while the others add, edit, delete and rename: the others converge without it, it catches up to their manifest, no deleted file returns, and the server says in sync only once it is. |

Env:

- `E2E_KEEP=1` keeps the VMs after a run; `E2E_SKIP_BUILD=1` reuses the last build.
- `E2E_BOAT_TYPE` (default `small`), `E2E_TTL_HOURS` (default 2: how long a
  VM lives if teardown never runs).
- `E2E_BIG_MB`, `E2E_RESUME_MB`, `E2E_RESUME_RATE`, `E2E_SPEED_MB` size the
  transfers in `sync` and `reconnect`.
- `E2E_INTERRUPT_MB`, `E2E_INTERRUPT_RATE`, `E2E_CUT_SECS` for `interrupt`.
- `E2E_SCALE_FILES`, `E2E_SCALE_MB_FILES`, `E2E_SCALE_BIG_MB` for `scale`.
- `E2E_POWERCUT_ROUNDS`, `E2E_POWERCUT_FILES`, `E2E_POWERCUT_MB_FILES` for `powercut`.
- `E2E_MULTI_ROUNDS` (default 8) for `multinode`.

## Open fds

`scale` and `multinode` sample each node's open fds, inotify instances and
inotify watches every 2 s and print the peaks. They fail when peak fds pass
`E2E_BUDGET_FDS` (default 128; the peak measured with a soft limit of 1024 was
71, on a tree of 1 MB files), when a node holds more than one inotify
instance, or when fds grow by more than `E2E_FD_GROWTH` (default 16) between
settled points or over a final `E2E_FD_IDLE` seconds (default 60).

## Timing budgets

`sync`, `scale` and `multinode` print a timing table and fail when a number is
over budget. The defaults come from a real run plus a margin on `small` VMs.
Set one in the environment to change it, or to empty to only report it.

| Env | Default | Measured |
| --- | --- | --- |
| `E2E_BUDGET_SYNC_LISTED` | 15 s, to node-a's own list | 4.4 to 5.1 s |
| `E2E_BUDGET_SYNC_FIRST` | 20 s, to node-b | 4.9 to 5.4 s |
| `E2E_BUDGET_SYNC_ALL` | 30 s | 5.6 to 6.3 s |
| `E2E_BUDGET_SYNC_MBPS` | at least 6 MB/s | 16.4 to 18.7 MB/s |
| `E2E_BUDGET_SCALE_FIRST` | 30 s | 6.6 to 15.9 s |
| `E2E_BUDGET_SCALE_ALL` | 240 s, each way | 29 to 92 s |
| `E2E_BUDGET_SCALE_DELETE_FIRST` | 20 s | 6.3 to 7.1 s |
| `E2E_BUDGET_SCALE_DELETE` | 30 s | 10.2 to 11.6 s |
| `E2E_BUDGET_SCALE_MBPS` | at least 5 MB/s | 6.1 to 17.4 MB/s |
| `E2E_BUDGET_MULTI_CONVERGE` | 90 s | 28.3 s |
| `E2E_BUDGET_MULTI_CATCHUP` | 60 s | 11.9 s |

Disks differ by VM, and that sets most of the spread in `SCALE_ALL`.

A new scenario is a `tests/e2e/<name>/run.sh` that sources `lib/common.sh`
and `lib/beep.sh`, calls `fresh_world`, and ends with `summary`; add its name
to `SCENARIOS` in `e2e.sh`.
