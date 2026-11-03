import test from "node:test";
import assert from "node:assert/strict";
import { BAND, keptShown, liveText, RATE, reveal, splitBand } from "../src/karaoke.ts";

test("a thought streams as plain text without Markdown marks", () => {
  assert.equal(liveText("\n\n**Checking 391**\n\n\n\nIt is `17 x 23`."), "Checking 391\n\nIt is 17 x 23.");
  assert.equal(liveText("## Plan\nRead __it__"), "Plan\nRead it");
});

test("the band reads at a steady pace and catches up when far behind", () => {
  // A short burst is read letter by letter at the steady pace.
  assert.equal(reveal(0, 60, 0.5, false), RATE / 2);
  let shown = 0;
  for (let i = 0; i < 60; i++) shown = reveal(shown, 60, 1 / 60, false);
  assert.ok(Math.abs(shown - RATE) < 1e-9, String(shown));
  // Far behind, it reads faster: a 300-character burst is read in a few seconds.
  shown = 0;
  let s = 0;
  while (shown < 300 && s < 10) { shown = reveal(shown, 300, 1 / 60, false); s += 1 / 60; }
  assert.ok(s < 5 && s > 300 / RATE / 3, String(s));
  // It never runs past the text.
  assert.equal(reveal(59.5, 60, 1, false), 60);
  // Very far behind, it jumps to the last stretch.
  assert.ok(reveal(0, 5000, 0, false) >= 4600);
  // Reduced motion shows it all at once.
  assert.equal(reveal(0, 300, 0.016, true), 300);
});

test("the band covers the newest characters read, the rest waits dimmed", () => {
  const text = "x".repeat(100) + "y".repeat(BAND);
  assert.deepEqual(splitBand(text, text.length), { old: "x".repeat(100), lit: "y".repeat(BAND), rest: "" });
  assert.deepEqual(splitBand("short", 3.7), { old: "", lit: "sho", rest: "rt" });
  assert.deepEqual(splitBand("short", 5, 0), { old: "short", lit: "", rest: "" });
});

test("text that grows keeps what was shown; changed text goes back to where it parts", () => {
  assert.equal(keptShown("Checking", "Checking the retry", 6.5), 6.5);
  assert.equal(keptShown("Checking the retry", "Checking a test", 15), 9);
});
