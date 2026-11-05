import test from "node:test";
import assert from "node:assert/strict";
import { type CopyRow, ago, copyKey, copyLists, discardLabel, lostLine, offlineLine } from "../src/agents/copy-state.ts";

const copy = (over: Partial<CopyRow> = {}): CopyRow => ({ agent: "codex", id: "s1", from: "laptop-id", from_name: "laptop", online: false, project: "acme/app", path: "", last_activity: 100, fetched_at: 200, size: 10, lost: null, ...over });

test("a session no machine lists and whose machine the mesh says is off is offered to continue", () => {
  const { offline } = copyLists([copy()], new Set());
  assert.deepEqual(offline.map(c => c.id), ["s1"]);
});

test("a session its machine still lists, or whose machine is on, is not offered", () => {
  assert.equal(copyLists([copy()], new Set(["s1"])).offline.length, 0);
  assert.equal(copyLists([copy({ online: true })], new Set()).offline.length, 0);
});

test("a version that lost is kept in its own list, whatever the machines do", () => {
  const lists = copyLists([copy({ lost: 5, online: true })], new Set(["s1"]));
  assert.equal(lists.offline.length, 0);
  assert.equal(lists.lost.length, 1);
});

test("copies come newest first", () => {
  const { offline } = copyLists([copy({ id: "old", last_activity: 1 }), copy({ id: "new", last_activity: 9 })], new Set());
  assert.deepEqual(offline.map(c => c.id), ["new", "old"]);
});

test("ages read in the unit that fits", () => {
  assert.equal(ago(30), "under 2 min");
  assert.equal(ago(180), "3 min");
  assert.equal(ago(7200), "2 h");
  assert.equal(ago(86400 * 4), "4 days");
});

test("the rows say how old the copy is, and that another machine went on", () => {
  assert.equal(offlineLine(copy({ fetched_at: 820 }), 1000), "laptop is offline. Copy from 3 min ago");
  assert.match(lostLine(), /continued this session later/);
});

test("a copy and a version of the same session have their own keys", () => {
  assert.notEqual(copyKey(copy()), copyKey(copy({ lost: 5 })));
});

test("discarding a version asks once before it throws it away", () => {
  assert.equal(discardLabel(false), "Discard");
  assert.notEqual(discardLabel(true), discardLabel(false));
});
