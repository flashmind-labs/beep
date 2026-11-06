import test from "node:test";
import assert from "node:assert/strict";
import { Closings, MARK_MS } from "../src/agents/closing-state.ts";

const s = { machine_id: "build-box", id: "a3f9c1" };

test("a closed session says closing at once and ended when its machine says so", () => {
  const c = new Closings();
  assert.equal(c.stateOf(s, 0), null);
  c.start(s, 0);
  assert.equal(c.stateOf(s, 1), "closing");
  // Another session ending is not this one.
  assert.ok(!c.ended("b7e2d0", 2));
  assert.equal(c.stateOf(s, 2), "closing");
  assert.ok(c.ended(s.id, 3));
  assert.equal(c.stateOf(s, 4), "ended");
  // Heard again, it is no news.
  assert.ok(!c.ended(s.id, 5));
});

test("a refused close leaves the row as it was", () => {
  const c = new Closings();
  c.start(s, 0);
  c.refused(s);
  assert.equal(c.stateOf(s, 1), null);
});

test("a mark gives way to the lists after a while", () => {
  const c = new Closings();
  c.start(s, 0);
  c.ended(s.id, 10);
  assert.equal(c.stateOf(s, 10 + MARK_MS), "ended");
  assert.equal(c.stateOf(s, 11 + MARK_MS), null);
});
