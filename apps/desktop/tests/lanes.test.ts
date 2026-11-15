import test from "node:test";
import assert from "node:assert/strict";
import { byLane, byProject, endedToday, keepOrder, laneOf, travelled } from "../src/center/lanes-state.ts";

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


test("Home keeps needs first by default and sorts projects by the chosen recency", () => {
  const cards = [
    { lane: "needs", project: "example/alpha", id: "question" },
    { lane: "working", project: "example/beta", id: "work" },
    { lane: "review", project: null, id: "other" },
  ] as const;
  assert.deepEqual(byProject(cards).map(row => row.project), ["example/alpha", "example/beta", null]);
  assert.deepEqual(byProject(cards, new Map([["example/beta", 200], ["example/alpha", 100]])).map(row => row.project), ["example/beta", "example/alpha", null]);
  assert.deepEqual(byProject(cards, new Map()).map(row => row.project), ["example/alpha", "example/beta", null]);
  assert.equal(cards[0].id, "question");
});

test("Home's rows keep their order while cards change lane, and a new project takes its sorted place", () => {
  const rows = ["alpha", "new", "beta", "gamma"].map(project => ({ project }));
  assert.deepEqual(keepOrder(rows, ["gamma", "gone", "beta", "alpha"]).map(row => row.project), ["gamma", "new", "beta", "alpha"]);
  assert.deepEqual(keepOrder(rows, []).map(row => row.project), ["alpha", "new", "beta", "gamma"]);
});
