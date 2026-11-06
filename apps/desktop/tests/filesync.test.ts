import test from "node:test";
import assert from "node:assert/strict";
import { conflictList, conflictPrompt, conflictTotal, fileSyncTile, machineMeta, originalOf, stateChip, stateNote, syncSummary, waitingList } from "../src/project/filesync.ts";
import type { MachineDetail, SyncDetail } from "../src/project/filesync.ts";

function machine(over: Partial<MachineDetail> = {}): MachineDetail {
  return {
    machine_id: "m1", machine_name: "build-box", folder: "/srv/project", status: "in_sync",
    files: 12480, bytes: 1_200_000_000, pending: 0, conflicts: [], error: null, hint: null,
    paused: false, progress: null, waiting: [], can_resolve: true, ...over,
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

test("the sidebar counts conflicts across projects, unread ones as none", () => {
  const a = detail([machine({ conflicts: ["a (conflict 11111111).txt", "b (conflict 22222222).txt"] })]);
  const b = detail([machine({ conflicts: ["c (conflict 33333333).txt"] }), machine({ machine_id: "m2", conflicts: ["c (conflict 33333333).txt"] })]);
  assert.equal(conflictTotal([a, b, null, undefined]), 3);
  assert.equal(conflictTotal([]), 0);
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
  assert.match(text, /update-ref -d only if I ask/);
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
