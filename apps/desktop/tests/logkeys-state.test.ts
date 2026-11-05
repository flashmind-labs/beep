import test from "node:test";
import assert from "node:assert/strict";
import { allOpen, commandRows, thoughtRows, turnsHolding } from "../src/agents/logkeys-state.ts";
import { splitTurns } from "../src/agents/agentturns.ts";

const log = [
  { kind: "user", text: "go" },
  { kind: "thought", text: "hm" },
  { kind: "tool", text: "cargo test [completed]" },
  { kind: "tool", text: "Read src/a.rs [completed]" },
  { kind: "agent", text: "done" },
  { kind: "user", text: "again" },
  { kind: "tool", text: "cargo build [completed]" },
];

test("commands and thoughts are found by their place in the session", () => {
  assert.deepEqual(commandRows(log, 10), [12, 16]);
  assert.deepEqual(thoughtRows(log, 10), [11]);
});

test("a toggle opens all when any is closed, else closes all", () => {
  assert.equal(allOpen([true, false, true]), true);
  assert.equal(allOpen([true, true]), false);
});

test("only turns holding the rows are named", () => {
  assert.deepEqual(turnsHolding(splitTurns(log), [1]), [0]);
});
