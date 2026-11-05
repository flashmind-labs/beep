import test from "node:test";
import assert from "node:assert/strict";
import { windowStart } from "../src/chat/history-state.ts";

const roles = ["user", "assistant", "user", "assistant", "approval", "scheduled", "assistant", "user", "assistant"];

test("a long chat draws from the start of its last prompts", () => {
  assert.equal(windowStart(roles, 1), 7);
  assert.equal(windowStart(roles, 2), 5);
  assert.equal(windowStart(roles, 3), 2);
});

test("a short chat draws whole", () => {
  assert.equal(windowStart(roles, 4), 0);
  assert.equal(windowStart(roles, 40), 0);
  assert.equal(windowStart([], 40), 0);
});
