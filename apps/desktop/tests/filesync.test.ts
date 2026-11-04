import test from "node:test";
import assert from "node:assert/strict";
import { conflictList, originalOf, waitingList } from "../src/project/filesync.ts";
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
