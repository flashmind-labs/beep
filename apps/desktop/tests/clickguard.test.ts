import test from "node:test";
import assert from "node:assert/strict";
import { swallowsClick } from "../src/ui/clickguard-state.ts";

test("a plain primary click passes", () => {
  assert.equal(swallowsClick({ button: 0, ctrlKey: false, native: false }), false);
});

test("a Ctrl+click and a secondary click are dropped", () => {
  assert.equal(swallowsClick({ button: 0, ctrlKey: true, native: false }), true);
  assert.equal(swallowsClick({ button: 2, ctrlKey: false, native: false }), true);
});

test("text fields, the terminal and links keep their Ctrl+click", () => {
  assert.equal(swallowsClick({ button: 0, ctrlKey: true, native: true }), false);
});
