import test from "node:test";
import assert from "node:assert/strict";
import { sessionOffers } from "../src/agents/sessionmenu-state.ts";

test("a running session can be stopped and closed", () => {
  assert.deepEqual(sessionOffers({ status: "working", external: false }), { stop: true, close: true, resume: false });
});

test("an idle session can be closed but has no turn to stop", () => {
  assert.deepEqual(sessionOffers({ status: "idle", external: false }), { stop: false, close: true, resume: false });
});

test("an ended session can be resumed", () => {
  assert.deepEqual(sessionOffers({ status: "ended", external: false }), { stop: false, close: false, resume: true });
});

test("a session started outside Beep is only followed", () => {
  for (const status of ["working", "idle", "ended"]) {
    assert.deepEqual(sessionOffers({ status, external: true }), { stop: false, close: false, resume: false });
  }
});
