import test from "node:test";
import assert from "node:assert/strict";
import { atBottom, BOTTOM_SLACK, keptScrollTop, logScroll } from "../src/logscroll.ts";

test("a reader at the bottom follows new entries", () => {
  assert.equal(logScroll(true, false), "bottom");
  assert.equal(logScroll(false, false), "keep");
});

test("opening or folding a row never jumps to the bottom", () => {
  // The regression: expanding Worked at the bottom of the log scrolled
  // past what it opened.
  assert.equal(logScroll(true, true), "keep");
  assert.equal(logScroll(false, true), "keep");
});

test("the bottom is the end of the box, or near it", () => {
  assert.equal(atBottom({ scrollTop: 600, scrollHeight: 1000, clientHeight: 400 }), true);
  assert.equal(atBottom({ scrollTop: 600 - BOTTOM_SLACK + 1, scrollHeight: 1000, clientHeight: 400 }), true);
  assert.equal(atBottom({ scrollTop: 600 - BOTTOM_SLACK, scrollHeight: 1000, clientHeight: 400 }), false);
  // A log that fits its box is at the bottom.
  assert.equal(atBottom({ scrollTop: 0, scrollHeight: 300, clientHeight: 400 }), true);
});

test("a kept row goes back to where it was on screen", () => {
  // The row was 120px from the top; after the redraw it is 180px down.
  assert.equal(keptScrollTop(500, 120, 180), 560);
  // It moved up: the log scrolls back, never above its top.
  assert.equal(keptScrollTop(500, 120, 20), 400);
  assert.equal(keptScrollTop(30, 120, 20), 0);
});
