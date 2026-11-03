import test from "node:test";
import assert from "node:assert/strict";
import { START, next } from "../src/addmachine-state.ts";

const created = { type: "created", token: "bk1-x", command: "beep-node setup bk1-x", expiresAt: 600_000 } as const;

test("a token waits, then expires on the tick that reaches it", () => {
  let s = next(START, created);
  assert.equal(s.step, "waiting");
  assert.equal(s.command, "beep-node setup bk1-x");
  assert.equal(next(s, { type: "tick", now: 599_999 }), s);
  s = next(s, { type: "tick", now: 600_000 });
  assert.deepEqual(s, { step: "expired" });
  assert.deepEqual(next(s, { type: "renew" }), START);
});

test("a found machine links only while waiting", () => {
  const machine = { id: "m", name: "build-box" };
  const s = next(next(START, created), { type: "found", machine });
  assert.deepEqual(s, { step: "linked", machine });
  // Neither a later tick nor a late token undoes it.
  assert.equal(next(s, { type: "tick", now: 10_000_000 }), s);
  assert.equal(next(s, created), s);
  assert.equal(next({ step: "expired" }, { type: "found", machine }).step, "expired");
});
