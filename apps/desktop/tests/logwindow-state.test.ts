import test from "node:test";
import assert from "node:assert/strict";
import { guess, shiftAbove, spacer, windowOf } from "../src/agents/logwindow-state.ts";

test("a turn never measured is as tall as the mean of those measured and shown", () => {
  assert.equal(guess([100, 200, 0], 80), 150);
  assert.equal(guess([], 80), 80);
});

test("only the turns that reach into the view and its margin are drawn", () => {
  const sizes = Array(5000).fill(100);
  assert.deepEqual(windowOf(sizes, 250_000, 251_600), { from: 2500, to: 2516 });
  assert.deepEqual(windowOf(sizes, -800, 800), { from: 0, to: 8 });
  assert.deepEqual(windowOf(sizes, 499_000, 501_000), { from: 4990, to: 5000 });
});

test("a turn cut by the view's edge is drawn", () => {
  assert.deepEqual(windowOf([100, 100, 100, 100], 150, 250), { from: 1, to: 3 });
});

test("a view past the end keeps the last turn, and an empty log draws none", () => {
  assert.deepEqual(windowOf([100, 100], 900, 1000), { from: 1, to: 2 });
  assert.deepEqual(windowOf([], 0, 1000), { from: 0, to: 0 });
});

test("a spacer stands for its turns less the one gap it takes", () => {
  const sizes = [112, 52, 212];
  assert.equal(spacer(sizes, 0, 2, 12), 152);
  assert.equal(spacer(sizes, 2, 2, 12), null);
  assert.equal(spacer([0, 0], 0, 2, 12), null);
});

test("turns that grow above the view move it; one in or below it does not", () => {
  assert.equal(shiftAbove([{ top: -500, was: 100, now: 340 }]), 240);
  assert.equal(shiftAbove([{ top: -50, was: 100, now: 340 }]), 0);
  assert.equal(shiftAbove([{ top: 200, was: 100, now: 40 }, { top: -300, was: 200, now: 150 }]), -50);
});
