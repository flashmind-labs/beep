import test from "node:test";
import assert from "node:assert/strict";
import { splice, endsForGood } from "../src/terminals.ts";

test("output already on the screen is skipped, and a gap is noted", () => {
  // Fresh: everything from the start.
  assert.deepEqual(splice(0, 0, 10), { skip: 0, gap: false });
  // A reattach resent bytes 5..15 while the screen had 10.
  assert.deepEqual(splice(10, 5, 10), { skip: 5, gap: false });
  // All of it was there already.
  assert.deepEqual(splice(20, 5, 10), { skip: 10, gap: false });
  // The node no longer kept bytes 10..30.
  assert.deepEqual(splice(10, 30, 4), { skip: 0, gap: true });
});

test("a terminal that exited or is gone is done; an offline one is not", () => {
  assert.ok(endsForGood("exited"));
  assert.ok(endsForGood("stopped"));
  assert.ok(endsForGood("gone"));
  assert.ok(!endsForGood("offline"));
  assert.ok(!endsForGood("running"));
});
