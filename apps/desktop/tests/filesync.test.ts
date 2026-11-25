import test from "node:test";
import assert from "node:assert/strict";
import { conflictList, conflictPrompt, fileSyncTile, machineMeta, originalOf, stateChip, stateNote, syncSummary, waitingList } from "../src/project/filesync.ts";
import type { MachineDetail, SyncDetail } from "../src/project/filesync.ts";

function machine(over: Partial<MachineDetail> = {}): MachineDetail {
  return {
    machine_id: "m1", machine_name: "build-box", folder: "/srv/project", status: "in_sync",
    files: 12480, bytes: 1_200_000_000, pending: 0, conflicts: [], error: null, hint: null,
    paused: false, progress: null, waiting: [], can_start_over: false, can_resolve: true, ...over,
  };
}

function detail(machines: MachineDetail[], over: Partial<SyncDetail> = {}): SyncDetail {
  return { project: "acme/widget", pinned: false, paused: false, machines, ...over };
}

test("a conflict copy names the file it stands beside", () => {
  assert.equal(originalOf("notes (conflict 1a2b3c4d).txt"), "notes.txt");
  assert.equal(originalOf("d.v2/Makefile (conflict abababab)"), "d.v2/Makefile");
  assert.equal(originalOf("docs/x (1).tar (conflict abababab).gz"), "docs/x (1).tar.gz");
  assert.equal(originalOf("plain.txt"), "plain.txt");
});

test("each conflict is listed once, settled by an online machine whose node can", () => {
  const copy = "plan (conflict 9fbd5479).md";
  const list = conflictList(detail([
    machine({ machine_id: "old", conflicts: [copy], can_resolve: false }),
    machine({ machine_id: "away", conflicts: [copy], status: "offline" }),
    machine({ machine_id: "new", conflicts: [copy] }),
  ]));
  assert.equal(list.length, 1);
  assert.equal(list[0].original, "plan.md");
  assert.deepEqual(list[0].on, ["old", "away", "new"]);
  assert.equal(list[0].resolver, "new");
  const none = conflictList(detail([machine({ conflicts: [copy], can_resolve: false })]));
  assert.equal(none[0].resolver, null);
});

test("waiting changes are listed once with the machines they wait on", () => {
  const list = waitingList(detail([
    machine({ machine_name: "a", waiting: ["src/main.rs", "README.md"] }),
    machine({ machine_name: "b", waiting: ["src/main.rs"] }),
  ]));
  assert.deepEqual(list, [{ path: "src/main.rs", machines: ["a", "b"] }, { path: "README.md", machines: ["a"] }]);
});

test("a conflict is fixed on the first online machine that has it", () => {
  const copy = "plan (conflict 9fbd5479).md";
  const list = conflictList(detail([
    machine({ machine_id: "away", conflicts: [copy], status: "offline" }),
    machine({ machine_id: "up", conflicts: [copy], can_resolve: false }),
  ]));
  assert.equal(list[0].holder, "up");
  assert.equal(conflictList(detail([machine({ conflicts: [copy], status: "offline" })]))[0].holder, null);
});

test("the file prompt names both files, merges, deletes the copy and does not commit", () => {
  const path = "docs/notes (conflict 1a2b3c4d).txt";
  const text = conflictPrompt({ path, original: originalOf(path) });
  assert.match(text, /Read docs\/notes\.txt and docs\/notes \(conflict 1a2b3c4d\)\.txt/);
  assert.match(text, /keeping both sides' intent, then delete docs\/notes \(conflict 1a2b3c4d\)\.txt/);
  assert.match(text, /Do not commit/);
  assert.match(text, /contradict, ask me/);
});

test("a git conflict is merged without rewriting history and the ref kept until asked", () => {
  const path = ".git/refs/beep-conflicts/heads/feat/x-5e6f7a8b";
  assert.equal(originalOf(path), ".git/refs/heads/feat/x");
  const text = conflictPrompt({ path, original: originalOf(path) });
  assert.match(text, /refs\/heads\/feat\/x at once.*refs\/beep-conflicts\/heads\/feat\/x-5e6f7a8b/);
  assert.match(text, /without rewriting history/);
  assert.match(text, /`git update-ref -d` only if I ask/);
});

test("a folder Syncthing has reads as waiting for it, not as failed", () => {
  const held = machine({ status: "syncthing", files: 0, bytes: 0, error: "Already synced by Syncthing (Work)" });
  assert.deepEqual(stateChip(held), { text: "waiting for Syncthing", wait: false });
  assert.deepEqual(machineMeta(held), []);
  assert.deepEqual(stateChip(machine({ status: "failed", error: "no room" })), { text: "not syncing", wait: true });
});

test("a machine that does not sync says why, and a missing folder says it comes back", () => {
  const refused = machine({ status: "failed", error: "/srv/.stignore has rules Beep cannot follow" });
  assert.equal(stateNote(refused), "/srv/.stignore has rules Beep cannot follow");
  assert.deepEqual(stateChip(machine({ status: "missing" })), { text: "folder missing", wait: true });
  assert.match(stateNote(machine({ status: "missing" })) || "", /makes it again and fills it/);
  assert.deepEqual(stateChip(machine({ status: "parent_missing" })), { text: "disk missing", wait: true });
  assert.equal(stateNote(machine()), null);
  assert.equal(stateNote(machine({ status: "failed", error: "not syncing: the folder changed or is not mounted", can_start_over: true })), "This folder was replaced, so sync stopped.");
});

test("machines Syncthing keeps are not counted as not syncing", () => {
  const st = (id: string) => machine({ machine_id: id, status: "syncthing" });
  assert.equal(syncSummary(detail([st("m1"), st("m2"), st("m3")])), "Syncthing keeps it in step");
  assert.equal(syncSummary(detail([st("m1"), machine({ machine_id: "m2", status: "offline" })])), "Syncthing keeps it in step · 1 offline");
  assert.equal(syncSummary(detail([machine(), st("m2")])), "in sync · 1 by Syncthing");
});

test("the header says in sync only when every machine is", () => {
  const refused = machine({ machine_id: "m2", status: "failed", error: "no" });
  const away = machine({ machine_id: "m3", status: "offline" });
  assert.equal(syncSummary(detail([machine(), machine({ machine_id: "m2" })])), "in sync");
  assert.equal(syncSummary(detail([machine(), refused, away])), "1 of 3 in sync · 1 not syncing · 1 offline");
  assert.equal(syncSummary(detail([refused])), "0 of 1 in sync · 1 not syncing");
  assert.equal(syncSummary(detail([machine({ status: "syncing" }), refused])), "1 syncing");
  assert.equal(syncSummary(detail([refused], { paused: true })), "paused on every machine");
  assert.equal(fileSyncTile(detail([machine(), refused])).count, "1 of 2 in sync · 1 not syncing");
});

test("a Windows folder shows without its prefix", () => {
  const tile = fileSyncTile(detail([machine({ machine_name: "win", folder: "\\\\?\\C:\\Users\\me\\widget" })]));
  assert.equal(tile.rows[0].title, "win: C:\\Users\\me\\widget");
});

test("a sync detail pushed during the initial read is not undone by its answer", async () => {
  const { makeFileSync } = await import("../src/project/filesync.ts");
  let finish!: (value: SyncDetail) => void;
  const initial = new Promise<SyncDetail>(done => { finish = done; });
  let reads = 0;
  const invoke = (async (command: string) => {
    if (command !== "project_sync_detail") return [];
    reads++;
    return initial;
  }) as Parameters<typeof makeFileSync>[0]["invoke"];
  const sync = makeFileSync({ invoke, redraw() {} });
  const first = sync.ensure("example");
  sync.take("example", detail([machine({ pending: 2 })]));
  finish(detail([machine({ pending: 0 })]));
  await first;
  assert.equal(reads, 1);
  assert.equal(sync.detail("example")!.machines[0].pending, 2);
  assert.equal(sync.ensure("example"), undefined, "a project read in this launch is not read again");
});

test("automatic Git divergence has a warning chip until repaired", () => {
  const failed = machine({ status: "failed", git_only: true, error: "Git commits differ; histories diverged; merge the peer commits to resume sync" });
  assert.deepEqual(stateChip(failed), { text: "Git sync needs review", wait: true });
  assert.deepEqual(stateChip({ ...failed, status: "in_sync", error: null }), { text: "in sync", wait: false });
  assert.deepEqual(stateChip({ ...failed, error: "Waiting for peer Git commits to arrive" }), { text: "not syncing", wait: true });
});
