import test from "node:test";
import assert from "node:assert/strict";
import { sessionOffers } from "../src/agents/sessionmenu-state.ts";

test("a running session can be stopped and closed", () => {
  assert.deepEqual(sessionOffers({ status: "working", external: false, agent: "claude" }), { stop: true, close: true, resume: false, delete: false });
});

test("an idle session can be closed but has no turn to stop", () => {
  assert.deepEqual(sessionOffers({ status: "idle", external: false, agent: "claude" }), { stop: false, close: true, resume: false, delete: false });
});

test("an ended session can be resumed", () => {
  assert.deepEqual(sessionOffers({ status: "ended", external: false, agent: "claude" }), { stop: false, close: false, resume: true, delete: true });
});

test("a session started outside Beep is only followed", () => {
  for (const status of ["working", "idle"]) {
    assert.deepEqual(sessionOffers({ status, external: true, agent: "claude" }), { stop: false, close: false, resume: false, delete: false });
  }
  assert.deepEqual(sessionOffers({ status: "ended", external: true, agent: "claude" }), { stop: false, close: false, resume: false, delete: true });
});

test("only an ended session of a file-based harness can be deleted", () => {
  for (const agent of ["claude", "codex", "pi"]) {
    assert.equal(sessionOffers({ status: "ended", external: false, agent }).delete, true);
  }
  for (const agent of ["flsh", "opencode", "gemini"]) {
    assert.equal(sessionOffers({ status: "ended", external: false, agent }).delete, false);
  }
});

test("a running session shows Close but not Delete", () => {
  for (const status of ["working", "idle", "asking", "starting", "waiting"]) {
    assert.equal(sessionOffers({ status, external: false, agent: "codex" }).delete, false);
  }
});

test("a machine only a server reaches is not offered Delete", () => {
  assert.equal(sessionOffers({ status: "ended", external: false, agent: "claude" }, false).delete, false);
});
