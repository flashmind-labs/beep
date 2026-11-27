import test from "node:test";
import assert from "node:assert/strict";
import { byLane, endedToday, laneOf, travelled } from "../src/stores/center/lanes-state.ts";

test("a session's status picks its lane", () => {
  assert.equal(laneOf("asking"), "needs");
  assert.equal(laneOf("starting"), "working");
  assert.equal(laneOf("waiting"), "working");
  assert.equal(laneOf("idle"), "review");
  assert.equal(laneOf("ended"), "done");
  assert.deepEqual(Object.keys(byLane([{ lane: laneOf("idle") }])), ["needs", "working", "review", "done"]);
});

test("Done today keeps sessions that ended since local midnight, the latest first", () => {
  const now = new Date(2026, 10, 12, 15).getTime(), hour = 3600_000;
  const sessions = [
    { id: "yesterday", status: "ended", last_activity: now - 16 * hour },
    { id: "early", status: "ended", last_activity: now - 14 * hour },
    { id: "late", status: "ended", last_activity: now - hour },
    { id: "idle", status: "idle", last_activity: now },
  ];
  assert.deepEqual(endedToday(sessions, now).map(s => s.id), ["late", "early"]);
});

test("only a card that moved travels, from where it was", () => {
  const before = new Map([["a", { x: 0, y: 40 }], ["b", { x: 300, y: 40 }], ["gone", { x: 0, y: 140 }]]);
  const after = new Map([["a", { x: 300, y: 140 }], ["b", { x: 300, y: 40 }], ["new", { x: 0, y: 40 }]]);
  assert.deepEqual([...travelled(before, after)], [["a", { dx: -300, dy: -100 }]]);
});

