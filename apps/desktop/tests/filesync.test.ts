import test from "node:test";
import assert from "node:assert/strict";
import { conflictList, fileSyncTile, machineMeta, originalOf, rateText, stateChip, syncPct, timeLeft, waitingList } from "../src/filesync.ts";
import type { MachineDetail, SyncDetail } from "../src/filesync.ts";

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

const moving = machine({
  machine_id: "m2", machine_name: "mac-box", status: "syncing",
  progress: { files_done: 142, files_total: 610, bytes_done: 48_000_000, bytes_total: 210_000_000, rate: 2_100_000, eta_secs: 78 },
});

test("rate and time left read as people say them", () => {
  assert.equal(rateText(2_100_000), "2.1 MB/s");
  assert.equal(rateText(0), "");
  assert.equal(timeLeft(null), "");
  assert.equal(timeLeft(20), "under a minute");
  assert.equal(timeLeft(78), "about 1 min");
  assert.equal(timeLeft(12 * 60), "about 12 min");
  assert.equal(timeLeft(2 * 3600 + 100), "about 2 h");
});

test("a syncing machine shows how far it is, its rate and time left", () => {
  assert.equal(syncPct(moving), 22);
  assert.deepEqual(machineMeta(moving), ["142 of 610 files", "48 MB of 210 MB", "2.1 MB/s", "about 1 min"]);
  assert.deepEqual(stateChip(moving), { text: "syncing 22%", wait: false });
  // Before the node reports progress, files and pending give the share.
  assert.equal(syncPct(machine({ status: "syncing", files: 30, pending: 70 })), 30);
  assert.equal(syncPct(machine()), null);
});

test("an idle machine shows its files, an offline one what waits", () => {
  assert.deepEqual(machineMeta(machine()), ["12,480 files", "1.2 GB"]);
  assert.deepEqual(machineMeta(machine({ status: "offline", pending: 3 })), ["3 changes waiting"]);
  assert.deepEqual(stateChip(machine({ status: "offline" })), { text: "offline", wait: false });
  assert.deepEqual(stateChip(machine({ paused: true, status: "syncing" })), { text: "paused", wait: false });
  assert.deepEqual(stateChip(machine({ status: "failed" })), { text: "failed", wait: true });
  assert.deepEqual(stateChip(machine({ conflicts: ["a (conflict 01020304).txt"] })), { text: "in sync", wait: true });
});

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

test("the tile counts what moves, then conflicts, and says when paused", () => {
  const tile = fileSyncTile(detail([machine(), moving]));
  assert.equal(tile.count, "1 syncing");
  assert.deepEqual(tile.rows.map(r => r.text), ["build-box", "mac-box"]);
  assert.equal(tile.rows[1].note, "syncing 22%");
  const conflicted = fileSyncTile(detail([machine({ conflicts: ["a (conflict 01020304).txt"] })]));
  assert.equal(conflicted.count, "1 conflict");
  assert.equal(conflicted.rows[0].chip!.cls, "wait");
  assert.equal(fileSyncTile(detail([machine({ paused: true })], { paused: true })).count, "paused");
  assert.equal(fileSyncTile(detail([])).empty, "No machine syncs it");
  assert.equal(fileSyncTile(undefined).empty, "Loading…");
});

test("before the detail loads, or from an older server, the tile uses the sync rows", () => {
  const tile = fileSyncTile(null, [{ machine_id: "m1", status: "syncing", files: 30, pending: 70 }], new Map([["m1", "gpu-box"]]));
  assert.equal(tile.count, "1 syncing");
  assert.equal(tile.rows[0].text, "gpu-box");
  assert.equal(tile.rows[0].note, "syncing 30%");
});
