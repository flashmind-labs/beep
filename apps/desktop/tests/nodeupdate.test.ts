import test from "node:test";
import assert from "node:assert/strict";
import { asksToStop, errorText, sessionsStop, updatedTo } from "../src/machines-state.ts";

test("the question counts the sessions that stop", () => {
  assert.equal(sessionsStop(1), "1 agent session is running and will stop.");
  assert.equal(sessionsStop(3), "3 agent sessions are running and will stop.");
});

test("a refusal over running sessions asks instead of failing", () => {
  const text = errorText("bad request: 2 agent sessions are running and will stop.");
  assert.equal(text, "2 agent sessions are running and will stop.");
  assert.ok(asksToStop(text));
  assert.ok(!asksToStop(errorText("bad request: build-box is offline.")));
});

test("a restarting node is back once online on the new version or newer", () => {
  assert.ok(!updatedTo({ online: false, version: "0.1.24" }, "0.1.24"));
  assert.ok(!updatedTo({ online: true, version: "0.1.20" }, "0.1.24"));
  assert.ok(updatedTo({ online: true, version: "0.1.24" }, "0.1.24"));
  assert.ok(updatedTo({ online: true, version: "0.1.25" }, "0.1.24"));
});
