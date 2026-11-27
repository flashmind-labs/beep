import { test } from "node:test";
import assert from "node:assert/strict";
import { waitedFor, waitLine } from "../src/stores/agents/waiting-state.ts";

test("waitLine is null with nothing running", () => {
  assert.equal(waitLine([]), null);
});

test("waitLine counts one kind", () => {
  assert.deepEqual(waitLine([{ target: { kind: "command" }, title: "Run tests", start: 5 }]),
    { label: "Waiting for 1 command", title: "Run tests", start: 5, target: { kind: "command" } });
  assert.equal(waitLine([{ target: { kind: "subagent", at: 3 }, title: "A", start: 1 }, { target: { kind: "subagent", at: 7 }, title: "B", start: 2 }])?.label, "Waiting for 2 subagents");
});

test("waitLine counts both kinds and names the oldest", () => {
  const line = waitLine([
    { target: { kind: "command" }, title: "Run tests", start: 30 },
    { target: { kind: "subagent", at: 4 }, title: "Upgrade all deps", start: 10 },
    { target: { kind: "subagent", at: 9 }, title: "Check docs", start: 20 },
  ]);
  assert.deepEqual(line, { label: "Waiting for 2 subagents and 1 command", title: "Upgrade all deps", start: 10, target: { kind: "subagent", at: 4 } });
});

test("waitedFor shows whole minutes from one on", () => {
  assert.equal(waitedFor(59_999), null);
  assert.equal(waitedFor(12 * 60_000 + 30_000), "12 min");
  assert.equal(waitedFor(60 * 60_000), "1 h");
  assert.equal(waitedFor(65 * 60_000), "1 h 5 min");
});
