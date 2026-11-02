import test from "node:test";
import assert from "node:assert/strict";
import { sizeLabel, linesLabel, withoutBeepBlock, appended, syncSummary, fileState, versionLine, agentName } from "../dist/instructions-state.js";

test("an agent file loses Beep's own block when imported", () => {
  const file = "# Mine\n- Run the checks.\n\n<!-- beep:start -->\n- Write plainly.\n<!-- beep:end -->\n\n## After\n";
  assert.equal(withoutBeepBlock(file), "# Mine\n- Run the checks.\n\n## After");
  assert.equal(withoutBeepBlock("<!-- beep:start -->\nx\n<!-- beep:end -->\n"), "");
  assert.equal(withoutBeepBlock("plain\n"), "plain");
  assert.equal(withoutBeepBlock(null), "");
});

test("adding a file's rules keeps mine first", () => {
  assert.equal(appended("# Mine\n\n", "- More\n"), "# Mine\n\n- More\n");
  assert.equal(appended("", "- More"), "- More\n");
  assert.equal(appended("# Mine", " "), "# Mine");
});

test("sizes, lines and versions read as the view says them", () => {
  assert.equal(sizeLabel("x".repeat(614)), "Markdown · 0.6 of 16 KB");
  assert.equal(linesLabel("a\nb\n"), "2 lines");
  assert.equal(linesLabel("a"), "1 line");
  assert.equal(linesLabel("  "), "empty");
  assert.equal(versionLine({ version: 8, note: "Restored version 3" }), "Version 8 · Restored version 3");
  assert.equal(versionLine(null), "Not saved yet");
  assert.equal(agentName("gemini"), "Gemini CLI");
});

test("a machine is synced when every agent file on it is", () => {
  const read = [
    { error: null, files: [{ synced: true }, { synced: true }] },
    { error: null, files: [{ synced: true }, { synced: false, text: "x" }] },
    { error: "offline", files: [] },
    { error: null, files: [] },
  ];
  assert.deepEqual(syncSummary(read), { machines: 2, synced: 1 });
  assert.equal(fileState({ synced: false, text: null }).text, "Not written yet");
  assert.equal(fileState({ synced: false, text: "x" }).cls, "warn");
});
