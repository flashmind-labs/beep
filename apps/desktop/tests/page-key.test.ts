import test from "node:test";
import assert from "node:assert/strict";
import { pageDirection } from "../src/agents/page-key.ts";

const key = (name: string) => ({ key: name, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, isComposing: false });

test("page keys scroll the conversation only without modifiers", () => {
  assert.equal(pageDirection(key("PageUp"), true), -1);
  assert.equal(pageDirection(key("PageDown"), true), 1);
  assert.equal(pageDirection(key("PageUp"), false), 0);
  assert.equal(pageDirection({ ...key("PageUp"), shiftKey: true }, true), 0);
  assert.equal(pageDirection({ ...key("PageUp"), isComposing: true }, true), 0);
});
