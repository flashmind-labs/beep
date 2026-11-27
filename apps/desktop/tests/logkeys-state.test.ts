import test from "node:test";
import assert from "node:assert/strict";
import { allOpen, commandRows, pageDirection, thoughtRows } from "../src/parts/session/logkeys-state.ts";

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

test("thought shortcuts act on group heads, separated by output", () => {
  const thought = { kind: "thought", text: "Thinking" };
  assert.deepEqual(thoughtRows([thought, thought, log[2], thought, thought], 10), [10, 13]);
});

test("a toggle opens all when any is closed, else closes all", () => {
  assert.equal(allOpen([true, false, true]), true);
  assert.equal(allOpen([true, true]), false);
});

const key = (name: string) => ({ key: name, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, isComposing: false });

test("page keys scroll the conversation only without modifiers", () => {
  assert.equal(pageDirection(key("PageUp"), true), -1);
  assert.equal(pageDirection(key("PageDown"), true), 1);
  assert.equal(pageDirection(key("PageUp"), false), 0);
  assert.equal(pageDirection({ ...key("PageUp"), shiftKey: true }, true), 0);
  assert.equal(pageDirection({ ...key("PageUp"), isComposing: true }, true), 0);
});
