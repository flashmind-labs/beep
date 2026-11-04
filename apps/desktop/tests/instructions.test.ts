import test from "node:test";
import assert from "node:assert/strict";
import { withoutBeepBlock, fileState, behind } from "../src/settings/instructions-state.ts";

test("an agent file loses Beep's own block when imported", () => {
  const file = "# Mine\n- Run the checks.\n\n<!-- beep:start -->\n- Write plainly.\n<!-- beep:end -->\n\n## After\n";
  assert.equal(withoutBeepBlock(file), "# Mine\n- Run the checks.\n\n## After");
  assert.equal(withoutBeepBlock("<!-- beep:start -->\nx\n<!-- beep:end -->\n"), "");
  assert.equal(withoutBeepBlock("plain\n"), "plain");
  assert.equal(withoutBeepBlock(null), "");
});

test("before the first save no file is out of sync", () => {
  // Nothing saved means nothing to write: neutral states, never a warning.
  assert.deepEqual(fileState({ synced: false, text: null }, false), { cls: "", text: "Not written yet" });
  assert.deepEqual(fileState({ synced: false, text: "# Mine\n" }, false), { cls: "", text: "Has its own text" });
  assert.equal(fileState({ synced: false, text: "# Mine\n" }, true).text, "Not synced");
});

test("write to all machines picks the machines that miss the newest version", () => {
  const file = (synced: boolean) => ({ agent: "claude", path: "~/.claude/CLAUDE.md", text: "x", synced });
  const m = (machine_id: string, files: ReturnType<typeof file>[], error: string | null = null) => ({ machine_id, machine_name: machine_id, files, error });
  assert.deepEqual(behind([
    m("build-box", [file(true), file(false)]),
    m("mac-box", [file(true)]),
    m("gpu-box", [], "offline"),
    m("win-box", []),
  ]), ["build-box"]);
  assert.deepEqual(behind(null), []);
});
