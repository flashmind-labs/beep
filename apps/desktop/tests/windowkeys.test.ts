import test from "node:test";
import assert from "node:assert/strict";
import { windowTakes } from "../src/windowkeys.ts";

test("⌘R in the window opens its Research page, not the pane's list", () => {
  assert.equal(windowTakes("r", "chat"), true);
  assert.equal(windowTakes("R", "agent"), true);
  assert.equal(windowTakes("r", undefined), true);
  // In a list of chats it renames one.
  assert.equal(windowTakes("r", "chats"), false);
});

test("the window keeps its palette and pages; the pane keeps the rest", () => {
  for (const key of ["k", ",", "m", "j", "l"]) assert.equal(windowTakes(key, "chat"), true, key);
  // ⌘M on the start form picks the model there.
  assert.equal(windowTakes("m", "agentnew"), false);
  assert.equal(windowTakes("n", "chat"), false);
  assert.equal(windowTakes("e", "chat"), false);
});
