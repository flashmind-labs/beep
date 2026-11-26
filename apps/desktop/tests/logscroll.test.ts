import test from "node:test";
import assert from "node:assert/strict";
import { followsAfterScroll, keptScrollTop, markPlace, logScroll, pinWait, returnScroll } from "../src/agents/logscroll.ts";

test("scrolling up from the end lets go at once, inside the slack too", () => {
  // The regression (#369): a move up of a few pixels still counted as at
  // the bottom, so the log was pinned back down until the reader's scroll
  // grew past the slack.
  const box = { scrollTop: 990, scrollHeight: 2000, clientHeight: 1000 };
  assert.equal(followsAfterScroll(true, 0, box), false);
  // Down, or still: it keeps following, and the bottom takes it back.
  assert.equal(followsAfterScroll(true, 100, { ...box, scrollTop: 950 }), true);
  assert.equal(followsAfterScroll(false, 100, box), true);
  assert.equal(followsAfterScroll(false, 500, { ...box, scrollTop: 600 }), false);
  // A bounce past the end settling back is not a move up.
  assert.equal(followsAfterScroll(true, 0, { ...box, scrollTop: 1000 }), true);
});

test("a pin to the end waits while the wheel turns", () => {
  assert.equal(pinWait(1000), 0);
  assert.equal(pinWait(1000, 900), 50);
  assert.equal(pinWait(1000, 800), 0);
});

test("opening or folding a row never jumps to the bottom", () => {
  // The regression: expanding Worked at the bottom of the log scrolled
  // past what it opened.
  assert.equal(logScroll(true, "toggle"), "keep");
  assert.equal(logScroll(false, "toggle"), "keep");
});

test("a log just opened stays at its end while older entries come in above", () => {
  // The regression (#381): opening a session reads older entries until the
  // oldest turn has its prompt, and that read held the row at the top in
  // place like a click does, so the log opened at its top.
  assert.equal(logScroll(true, "older"), "bottom");
  assert.equal(logScroll(true, "news"), "bottom");
  // A reader who scrolled up keeps their row.
  assert.equal(logScroll(false, "older"), "keep");
  assert.equal(logScroll(false, "news"), "keep");
});

test("a log put back goes to its end unless the reader had scrolled up", () => {
  const box = { scrollTop: 0, scrollHeight: 3000, clientHeight: 800 };
  assert.equal(returnScroll(box, 1200), 3000);
  assert.equal(returnScroll({ ...box, stuck: true }, 1200), 3000);
  assert.equal(returnScroll({ ...box, stuck: false }, 1200), 1200);
});

test("a kept row goes back to where it was on screen", () => {
  // The row was 120px from the top; after the redraw it is 180px down.
  assert.equal(keptScrollTop(500, 120, 180), 560);
  // It moved up: the log scrolls back, never above its top.
  assert.equal(keptScrollTop(500, 120, 20), 400);
  assert.equal(keptScrollTop(30, 120, 20), 0);
});


test("a layout adjustment after sending attachments preserves following the bottom", () => {
  const adjusted = { scrollTop: 800, scrollHeight: 2300, clientHeight: 1100 };
  assert.equal(followsAfterScroll(true, 0, adjusted, true), true);
  assert.equal(followsAfterScroll(true, 0, adjusted, false), false);
  assert.equal(followsAfterScroll(false, 0, adjusted, true), false);
});

test("a move up that arrives with a pin to the end still lets go", () => {
  // Older entries came in above and the log was pinned (gap 0); the reader
  // scrolled up 300 px in the same event. The raw scrollTop is far past the
  // one last seen, but the gap grew.
  const box = { scrollTop: 9700, scrollHeight: 11000, clientHeight: 1000 };
  assert.equal(followsAfterScroll(true, 0, box), false);
});

test("the place is noted only for a box in the page", () => {
  const box: { scrollTop: number; scrollHeight: number; clientHeight: number; top?: number; gap?: number } = { scrollTop: 500, scrollHeight: 2000, clientHeight: 1000 };
  markPlace(box);
  assert.deepEqual([box.top, box.gap], [500, 500]);
  Object.assign(box, { scrollTop: 0, clientHeight: 0 });
  markPlace(box);
  assert.deepEqual([box.top, box.gap], [500, 500]);
});
