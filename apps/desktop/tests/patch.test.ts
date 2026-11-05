import test from "node:test";
import assert from "node:assert/strict";
import { patchSteps, patchedSigs } from "../src/ui/patch-state.ts";

test("a redraw keeps the children that did not change", () => {
  const steps = patchSteps(["a", "b", "c"], ["a", "x", "c"], () => false);
  assert.deepEqual(steps, ["keep", "replace", "keep"]);
});

test("a redraw with the same markup touches nothing", () => {
  assert.deepEqual(patchSteps(["a", "b"], ["a", "b"], () => true), ["keep", "keep"]);
});

test("a child with a request in flight stays and is tried again next time", () => {
  const steps = patchSteps(["a", "b"], ["a", "x"], i => i === 1);
  assert.deepEqual(steps, ["keep", "keep"]);
  assert.deepEqual(patchedSigs(["a", "b"], ["a", "x"], steps), ["a", "b"]);
});

test("new children are added and the signatures follow", () => {
  const steps = patchSteps(["a"], ["a", "b", "c"], () => false);
  assert.deepEqual(steps, ["keep", "add", "add"]);
  assert.deepEqual(patchedSigs(["a"], ["a", "b", "c"], steps), ["a", "b", "c"]);
});
