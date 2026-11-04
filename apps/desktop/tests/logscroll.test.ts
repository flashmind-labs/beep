import test from "node:test";
import assert from "node:assert/strict";
import { keptScrollTop, logScroll } from "../src/agents/logscroll.ts";

test("opening or folding a row never jumps to the bottom", () => {
  // The regression: expanding Worked at the bottom of the log scrolled
  // past what it opened.
  assert.equal(logScroll(true, true), "keep");
  assert.equal(logScroll(false, true), "keep");
});

test("a kept row goes back to where it was on screen", () => {
  // The row was 120px from the top; after the redraw it is 180px down.
  assert.equal(keptScrollTop(500, 120, 180), 560);
  // It moved up: the log scrolls back, never above its top.
  assert.equal(keptScrollTop(500, 120, 20), 400);
  assert.equal(keptScrollTop(30, 120, 20), 0);
});
