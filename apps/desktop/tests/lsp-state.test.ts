import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  formatLocationLabel,
  parseEditorSettings,
  parseLspResult,
  resolveVimSync,
  wordSpanAt,
} from "../src/stores/agentpane/lsp-state.ts";
import { primaryModifier } from "../src/ui/platform.ts";

describe("primaryModifier", () => {
  it("uses Cmd on Apple and Ctrl elsewhere without extra modifiers", () => {
    assert.equal(primaryModifier({ metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }, true), true);
    assert.equal(primaryModifier({ metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }, true), false);
    assert.equal(primaryModifier({ metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }, false), true);
    assert.equal(primaryModifier({ metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }, false), false);
    assert.equal(primaryModifier({ metaKey: true, ctrlKey: false, altKey: false, shiftKey: true }, true), false);
    assert.equal(primaryModifier({ metaKey: false, ctrlKey: true, altKey: true, shiftKey: false }, false), false);
  });
});

describe("wordSpanAt", () => {
  it("finds identifier boundaries and ignores punctuation, whitespace, and past-end columns", () => {
    const line = "  const result = compute_sum(alpha, beta)";
    assert.deepEqual(wordSpanAt(line, 19), { from: 17, to: 28 });
    assert.equal(wordSpanAt(line, 28), null);
    assert.equal(wordSpanAt(line, line.length), null);
    assert.equal(wordSpanAt(line, 0), null);
    assert.equal(wordSpanAt(line, 15), null);
    assert.equal(wordSpanAt("", 0), null);
  });
});

describe("resolveVimSync", () => {
  it("prefers mesh state once saved and migrates local state otherwise", () => {
    assert.deepEqual(resolveVimSync(null, true), { enabled: true, writeLocal: true, writeRemote: false });
    assert.deepEqual(resolveVimSync("false", true), { enabled: true, writeLocal: true, writeRemote: false });
    assert.deepEqual(resolveVimSync("true", true), { enabled: true, writeLocal: false, writeRemote: false });
    assert.deepEqual(resolveVimSync("true", null), { enabled: true, writeLocal: false, writeRemote: true });
    assert.deepEqual(resolveVimSync("false", null), { enabled: false, writeLocal: false, writeRemote: true });
    assert.deepEqual(resolveVimSync(null, null), { enabled: false, writeLocal: false, writeRemote: false });
  });
});

describe("parseLspResult and parseEditorSettings", () => {
  it("validates IPC responses safely", () => {
    const parsed = parseLspResult({
      locations: [
        { path: "src/main.rs", inside_root: true, line: 9, column: 4 },
        { path: 123, line: 1, column: 1 },
      ],
      note: null,
    });
    assert.equal(parsed.note, null);
    assert.equal(parsed.locations.length, 1);
    assert.equal(formatLocationLabel(parsed.locations[0]), "src/main.rs:10");

    assert.deepEqual(parseLspResult({ locations: [], note: "No Rust language server found." }), {
      locations: [],
      note: "No Rust language server found.",
    });
    assert.deepEqual(parseEditorSettings({ vim: true }), { vim: true });
    assert.deepEqual(parseEditorSettings({ vim: null }), { vim: null });
    assert.deepEqual(parseEditorSettings("bad"), { vim: null });
  });
});
