import test from "node:test";
import assert from "node:assert/strict";
import { BAND, firstShown, keptShown, liveText, reveal, splitBand } from "../src/karaoke.ts";

test("a thought streams as plain text without Markdown marks", () => {
  assert.equal(liveText("\n\n**Checking 391**\n\n\n\nIt is `17 x 23`."), "Checking 391\n\nIt is 17 x 23.");
  assert.equal(liveText("## Plan\nRead __it__"), "Plan\nRead it");
});

test("the reveal keeps up with a burst and never runs past the text", () => {
  // A burst of 120 characters is shown within about a read of the session.
  let shown = 0;
  for (let i = 0; i < 90; i++) shown = reveal(shown, 120, 1 / 60, false);
  assert.ok(shown > 105 && shown <= 120, String(shown));
  for (let i = 0; i < 60; i++) shown = reveal(shown, 120, 1 / 60, false);
  assert.equal(shown, 120);
  // A little text left still moves at a readable pace.
  assert.ok(reveal(0, 10, 0.1, false) >= 3);
  // Far behind, it jumps to the last stretch.
  assert.ok(reveal(0, 5000, 0, false) >= 4400);
  // Reduced motion shows it all at once.
  assert.equal(reveal(0, 300, 0.016, true), 300);
});

test("the band covers the newest characters revealed", () => {
  const text = "x".repeat(100) + "y".repeat(BAND);
  assert.deepEqual(splitBand(text, text.length), { old: "x".repeat(100), lit: "y".repeat(BAND) });
  assert.deepEqual(splitBand("short", 3.7), { old: "", lit: "sho" });
  assert.deepEqual(splitBand("short", 5, 0), { old: "short", lit: "" });
});

test("text that grows keeps what was shown; changed text goes back to where it parts", () => {
  assert.equal(keptShown("Checking", "Checking the retry", 6.5), 6.5);
  assert.equal(keptShown("Checking the retry", "Checking a test", 15), 9);
  assert.equal(firstShown(50), 0);
  assert.equal(firstShown(1000), 880);
});
