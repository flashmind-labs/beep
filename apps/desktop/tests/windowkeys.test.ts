import test from "node:test";
import assert from "node:assert/strict";
import { windowTakes } from "../src/ui/windowkeys.ts";

test("⌘R in the window opens its Research page, not the pane's list", () => {
  assert.equal(windowTakes("r", "chat"), true);
  assert.equal(windowTakes("R", "agent"), true);
  assert.equal(windowTakes("r", undefined), true);
  // In a list of chats it renames one.
  assert.equal(windowTakes("r", "chats"), false);
});
