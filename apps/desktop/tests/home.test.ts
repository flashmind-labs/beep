import test from "node:test";
import assert from "node:assert/strict";
import { type MacControl, machineDisk, machineMemory, machineNow, machineProjects, projectsSummary, machinesReach, thisMacRow } from "../src/parts/machines/machine-state.ts";
import { byRecent, remotePath, runningKey, runningLine, runningWhen, sameSession } from "../src/stores/agents/running-state.ts";

test("a machine's synced projects: only its own, the detail while it agrees, what needs a look first", () => {
  const row = (project: string, machine_id: string, status: string) => ({ project, machine_id, status, files: 90, pending: 10, error: null });
  const rows = [row("done", "m", "in_sync"), row("stale", "m", "in_sync"), row("moving", "m", "syncing"), row("elsewhere", "other", "syncing")];
  const base = { machine_id: "m", machine_name: "M", folder: "", bytes: 0, files: 90, pending: 0, conflicts: [], error: null, hint: null, paused: false, progress: null, waiting: [], can_start_over: false, can_resolve: false };
  const detail = (status: string, more = {}) => ({ project: "", pinned: false, paused: false, machines: [{ ...base, status, ...more }] });
  const details: Record<string, ReturnType<typeof detail>> = {
    stale: detail("syncing"),
    moving: detail("syncing", { progress: { files_done: 1, files_total: 4, bytes_done: 50, bytes_total: 100, rate: 2000000, eta_secs: 30 } }),
  };
  const out = machineProjects("m", rows, p => details[p]);
  assert.deepEqual(out.map(p => p.project), ["moving", "done", "stale"]);
  assert.equal(out[0].pct, 50);
  assert.deepEqual(out[0].meta, ["1 of 4 files", "50 B of 100 B", "2.0 MB/s", "under a minute"]);
  assert.equal(out[2].pct, null, "a row that moved on wins over its older detail");
  assert.equal(out[2].chip.text, "In sync");
});

test("Running now puts the session that moved last first", () => {
  const agent = (id: string, started_at: number, last_activity: number) => ({ machine_id: "m", id, started_at, last_activity });
  const rows = [agent("old", 10, 20), agent("new", 5, 40), agent("quiet", 25, 0)];
  assert.deepEqual(byRecent(rows).map(runningKey), ["agent:m:new", "agent:m:quiet", "agent:m:old"]);
  assert.equal(runningKey(rows[0]), "agent:m:old", "the list it was given stays as it was");
});

test("a running agent shows its title, then agent, repo and machine", () => {
  const r = { agent: "claude", title: "Fix the build", cwd: "/srv/project/widget-fix/src", machine_id: "m1", machine_name: "build-box" };
  assert.deepEqual(runningLine(r), { title: "Fix the build", sub: "src · build-box" });
  const projects = [
    { name: "org/widget", machine_id: "m1", folder: "/srv/project/widget-fix" },
    { name: "org/other", machine_id: "m2", folder: "/srv/project/widget-fix/src" },
  ];
  assert.deepEqual(runningLine(r, projects), { title: "Fix the build", sub: "widget · build-box" });
  assert.deepEqual(runningLine({ ...r, title: null, cwd: "" }), { title: "Claude Code", sub: "build-box" });
  assert.deepEqual(runningLine({ ...r, account: "Work" }), { title: "Fix the build", sub: "src · build-box · Work" });
});

test("a remote's path drops the host, the port and .git", () => {
  assert.equal(remotePath("ssh://git@git.example:2222/group/sub/repo.git"), "group/sub/repo");
  assert.equal(remotePath("https://github.com/org/repo/"), "org/repo");
  assert.equal(remotePath("https://github.com/org"), null);
  assert.equal(remotePath("/srv/project/repo"), null);
  assert.equal(remotePath(null), null);
});

test("This computer gets a row until the mesh lists it", () => {
  const mac = (over: Partial<MacControl> = {}): Omit<MacControl, "permissions"> => ({ id: "mac-key", name: "Studio", enabled: true, status: "connecting", error: null, ...over });
  assert.equal(thisMacRow(mac(), [{ id: "mac-key" }]), null);
  assert.equal(thisMacRow(mac(), null), null);
  assert.equal(thisMacRow(null, []), null);
  assert.equal(thisMacRow(mac({ status: "unlinked" }), []), null);
  const off = thisMacRow(mac({ enabled: false, status: "off" }), [{ id: "other" }]);
  assert.equal(off!.state, "Off");
  assert.equal(off!.action, "Add");
  assert.equal(off!.name, "This computer");
  assert.equal(off!.sub, "Studio");
  const failed = thisMacRow(mac({ status: "failed", error: "connect timed out" }), []);
  assert.equal(failed!.state, "connect timed out");
  assert.match(failed!.title, /connect timed out/);
  assert.equal(thisMacRow(mac({ status: "connected" }), [])!.state, "Connecting");
});

test("a Running row shows time since last activity between turns", t => {
  const now = 1_800_000_000_000;
  t.mock.method(Date, "now", () => now);
  const agent = { status: "idle", started_at: now - 86_400_000, last_activity: now - 180_000 };
  assert.equal(runningWhen({ ...agent, status: "working" }), "now");
  assert.equal(runningWhen(agent), "3m ago");
  assert.equal(runningWhen({ ...agent, last_activity: now - 7_200_000 }), "2h ago");
  assert.equal(runningWhen({ ...agent, last_activity: now }), "now");
  assert.equal(runningWhen({ ...agent, last_activity: 0 }), "1d ago");
});

test("the disk column shows the other volume with the most free", () => {
  const stats = { disks: [
    { path: "C:\\Users\\someone", free_bytes: 2_500_000_000, total_bytes: 254_000_000_000 },
    { path: "E:\\", free_bytes: 7_000_000_000, total_bytes: 100_000_000_000 },
    { path: "D:\\", free_bytes: 512_000_000_000, total_bytes: 512_000_000_000 },
  ] };
  const d = machineDisk({ online: true, stats });
  assert.equal(d.text, "2.5 GB free");
  assert.equal(d.low, true);
  assert.equal(d.other, "D: 512 GB free");
  assert.equal(machineDisk({ online: true, stats: { disks: stats.disks.slice(0, 1) } }).other, "");
  assert.equal(machineDisk({ online: true, stats: { disks: [stats.disks[0], { path: "/mnt/data", free_bytes: 1, total_bytes: 2 }] } }).other, "data 1 B free");
});

test("disk and memory give the used fraction, and a machine says what it does now", () => {
  const disk = machineDisk({ online: true, stats: { disks: [{ path: "/srv", free_bytes: 250, total_bytes: 1000 }] } });
  assert.equal(disk.used, 0.75);
  assert.equal(machineDisk({ online: true, stats: null }).used, null);
  assert.equal(machineMemory({ online: true, stats: { memory_used_mb: 2048, memory_total_mb: 8192, load: null } }).used, 0.25);
  assert.equal(machineMemory({ online: false, stats: null }).used, null);
  const m = { online: true, agents: 0, can_host: true, connecting: false, seen_at: Date.now() - 2 * 3_600_000 };
  assert.equal(machineNow(m), "idle");
  assert.equal(machineNow({ ...m, agents: 2 }), "2 agents");
  assert.equal(machineNow({ ...m, can_host: false }), "controller");
  assert.equal(machineNow({ ...m, online: false }), "offline, last seen 2 h ago");
  assert.equal(machineNow({ ...m, online: false, connecting: true }), "connecting");
});

test("a folded project list counts its states, most common first", () => {
  const p = (text: string) => ({ chip: { text, wait: false } });
  assert.equal(projectsSummary([p("In sync"), p("In sync")]), "all in sync");
  assert.equal(projectsSummary([p("Paused"), p("In sync"), p("In sync"), p("Syncing 40%")]), "2 in sync · 1 paused · 1 syncing");
});

test("the open session is the same only on its machine and agent", () => {
  const open = { machine_id: "m1", agent: "codex", session: "s1" };
  assert.equal(sameSession(open, { machine_id: "m1", agent: "codex", id: "s1" }), true);
  assert.equal(sameSession(open, { machine_id: "m2", agent: "codex", id: "s1" }), false);
  assert.equal(sameSession(open, { machine_id: "m1", agent: "claude", id: "s1" }), false);
  assert.equal(sameSession(open, { machine_id: "m1", agent: "codex", id: "s2" }), false);
  assert.equal(sameSession(null, { machine_id: "m1", agent: "codex", id: "s1" }), false);
});

test("telemetry replaces only its machine stats and preserves samples newer than a replay", async () => {
  const { applyTelemetry } = await import("../src/stores/center/home-state.ts");
  const sample = (id: string, sampled_at: number, load: number) => ({ id, sampled_at, disks: [], memory_used_mb: 100, memory_total_mb: 200, load });
  const machine = (id: string, sampled_at: number) => ({ id, name: id, online: true, seen_at: 0, version: "", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: sample(id, sampled_at, 1), connecting: false });
  const home = { machines: [machine("machine-a", 10), machine("machine-b", 30)], running: [], needs_you: [], coming_up: [], updates: [], projects: [] };
  const updated = applyTelemetry(home, { stats: [sample("machine-a", 20, 2), sample("machine-b", 20, 0)], traffic: [] });
  assert.equal(updated.machines![0].stats!.load, 2);
  assert.equal(updated.machines![0].online, true);
  assert.equal(updated.machines![1], home.machines[1], "older snapshots cannot replace a newer sample");
  assert.equal(updated.running, home.running);
  assert.equal(updated.projects, home.projects);
  assert.equal(home.machines[0].stats.load, 1, "the prior Home response stays untouched");
});

test("Home's cards follow the machine list for names and removal", async () => {
  const { withListedMachines } = await import("../src/stores/center/home-state.ts");
  const card = (id: string, name: string) => ({ id, name, online: true, seen_at: 0, version: "", env: null, apps: [], gpus: [], capabilities: [], agents: 3, stats: null, connecting: false });
  const home = { machines: [card("a", "Alpha"), card("b", "Beta")], running: [], needs_you: [], projects: [] };
  const none = new Set<string>();

  assert.equal(withListedMachines(home, [{ id: "a", name: "Alpha" }, { id: "b", name: "Beta" }], none), home);
  assert.equal(withListedMachines(home, null, none), home);

  const renamed = withListedMachines(home, [{ id: "a", name: "Alpha" }, { id: "b", name: "Second" }], none);
  assert.equal(renamed.machines?.[0], home.machines[0]);
  assert.equal(renamed.machines?.[1]?.name, "Second");
  assert.equal(renamed.machines?.[1]?.agents, 3, "the card keeps what only Home reads");

  // A removal made here hides the card even while the list and Home still carry it.
  const gone = new Set(["a"]);
  assert.deepEqual(withListedMachines(home, [{ id: "b", name: "Beta" }], gone).machines?.map(m => m.id), ["b"]);
  assert.deepEqual(withListedMachines(home, [{ id: "a", name: "Alpha" }, { id: "b", name: "Beta" }], gone).machines?.map(m => m.id), ["b"]);
});

test("a machine only Home's push names keeps its card, before and after the list has it", async () => {
  const { withListedMachines } = await import("../src/stores/center/home-state.ts");
  const card = (id: string, name: string) => ({ id, name, online: true, seen_at: 0, version: "", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: null, connecting: false });
  const home = { machines: [card("a", "Alpha"), card("b", "Beta")], running: [], needs_you: [], projects: [] };
  const none = new Set<string>();

  assert.deepEqual(withListedMachines(home, [], none).machines?.map(m => m.id), ["a", "b"]);
  assert.deepEqual(withListedMachines(home, [{ id: "a", name: "Alpha" }], none).machines?.map(m => m.id), ["a", "b"]);
  assert.equal(withListedMachines(home, [{ id: "a", name: "Alpha" }, { id: "b", name: "Beta" }], none), home);
});

test("an empty list says connecting until a machine that runs agents answers", () => {
  const host = (online: boolean, connecting: boolean) => ({ online, connecting, can_host: true });
  const phone = { online: true, connecting: false, can_host: false };
  assert.equal(machinesReach(null), "connecting");
  assert.equal(machinesReach([phone, host(false, true), host(false, false)]), "connecting");
  assert.equal(machinesReach([phone, host(false, true), host(true, false)]), "reached");
  assert.equal(machinesReach([phone, host(false, false)]), "offline");
  assert.equal(machinesReach([phone]), "reached");
});

test("agent-started replaces the start placeholder in Home and sessions without duplicating", async () => {
  const { startedInHome } = await import("../src/stores/center/home-state.ts");
  const { replaceStartedSession } = await import("../src/stores/agents/starting-state.ts");

  const startRow = {
    kind: "agent" as const, machine_id: "m1", machine_name: "build-box", id: "start:1",
    agent: "claude", title: "New prompt", cwd: "/srv/project", status: "starting",
    doing: "Starting Claude Code on build-box", started_at: 10, last_activity: 10, external: false,
  };
  const real = {
    machine_id: "m1", machine_name: "build-box", id: "real-1", agent: "claude",
    title: "New prompt", cwd: "/srv/project", status: "working" as const, doing: null,
    started_at: 10, last_activity: 12, external: false, question: null, subagents: 0,
    assistant: false, persona: null,
  };
  const home = { machines: [], running: [startRow], needs_you: [], projects: [] };
  const replaced = startedInHome(home, "start:1", real);
  assert.deepEqual(replaced.running?.map(r => [r.id, r.status]), [["real-1", "working"]]);

  // If an `agents` push already inserted `real-1` beside `start:1`, `start:1` is dropped cleanly.
  const both = { ...home, running: [{ ...real, kind: "agent" as const }, startRow] };
  assert.deepEqual(startedInHome(both, "start:1", real).running?.map(r => r.id), ["real-1"]);

  const listed = [{ ...startRow, status: "starting" as const, question: null, subagents: 0, assistant: false, persona: null }];
  assert.deepEqual(replaceStartedSession(listed, "start:1", real).map(r => r.id), ["real-1"]);
  assert.deepEqual(replaceStartedSession([real, listed[0]], "start:1", real).map(r => r.id), ["real-1"]);
});
