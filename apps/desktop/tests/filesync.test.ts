import test from "node:test";
import assert from "node:assert/strict";
import { conflictList, conflictPrompt, conflictTotal, machineMeta, originalOf, stateChip, waitingList } from "../src/project/filesync.ts";
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

test("a folder Syncthing has reads as synced by it, not as failed", () => {
  const held = machine({ status: "failed", files: 0, bytes: 0, error: "Already synced by Syncthing (Work)" });
  assert.deepEqual(stateChip(held), { text: "Synced by Syncthing", wait: false });
  assert.deepEqual(machineMeta(held), []);
  assert.deepEqual(stateChip(machine({ status: "failed", error: "no room" })), { text: "failed", wait: true });
});
