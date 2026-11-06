import test from "node:test";
import assert from "node:assert/strict";
import { KINDS, START, inviteArgs, joinedNote, next, withSearch } from "../src/machines/addmachine-state.ts";

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

test("the setup command names the search folder only when it is not home", () => {
  for (const home of ["~/", " ~ ", ""]) assert.equal(withSearch("beep-node setup bk1-x", home), "beep-node setup bk1-x");
  assert.equal(withSearch("beep-node setup bk1-x", "~/Projects"), "beep-node setup bk1-x --search ~/Projects");
  assert.equal(withSearch("beep-node setup bk1-x", "~/My Code"), "beep-node setup bk1-x --search '~/My Code'");
});

test("a container ticket is a temporary node's", () => {
  assert.deepEqual(KINDS.map(k => k.kind), ["node", "container", "desktop"]);
  assert.deepEqual(inviteArgs("node"), { kind: "Node", temporary: false });
  assert.deepEqual(inviteArgs("container"), { kind: "Node", temporary: true });
  assert.deepEqual(inviteArgs("desktop"), { kind: "Desktop", temporary: false });
  assert.equal(joinedNote("container"), "It shows under Machines until it stops.");
  assert.equal(joinedNote("node"), "It shows under Machines and can run agents now.");
});
