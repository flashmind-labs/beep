import test from "node:test";
import assert from "node:assert/strict";
import { followsAfterScroll, keptScrollTop, logScroll, pinWait } from "../src/agents/logscroll.ts";

test("scrolling up from the end lets go at once, inside the slack too", () => {
  // The regression (#369): a move up of a few pixels still counted as at
  // the bottom, so the log was pinned back down until the reader's scroll
  // grew past the slack.
  const box = { scrollTop: 990, scrollHeight: 2000, clientHeight: 1000 };
  assert.equal(followsAfterScroll(true, 1000, box), false);
  // Down, or still: it keeps following, and the bottom takes it back.
  assert.equal(followsAfterScroll(true, 900, { ...box, scrollTop: 950 }), true);
  assert.equal(followsAfterScroll(false, 900, box), true);
  assert.equal(followsAfterScroll(false, 500, { ...box, scrollTop: 600 }), false);
  // A bounce past the end settling back is not a move up.
  assert.equal(followsAfterScroll(true, 1030, { ...box, scrollTop: 1000 }), true);
});

test("a pin to the end waits while the wheel turns", () => {
  assert.equal(pinWait(1000), 0);
  assert.equal(pinWait(1000, 900), 50);
  assert.equal(pinWait(1000, 800), 0);
});

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
