import test from "node:test";
import assert from "node:assert/strict";
import { groupThoughts, mergedThought } from "../src/stores/agents/thoughts-state.ts";

const thought = (text: string, ms: number | null = null, html?: string) => ({ kind: "thought", text, ms, html });

test("adjacent thoughts share a stable row and preserve tool groups", () => {
  const entries = [thought("First", 42000), thought("Second", 42000),
    { kind: "tool", text: "Read a file" }, { kind: "tool", text: "Read another file" }, thought("Third")];
  const groups = [[10], [11], [12, 13], [14]];
  assert.deepEqual(groupThoughts(entries, groups, 10), [[10, 11], [12, 13], [14]]);
  assert.deepEqual(groups, [[10], [11], [12, 13], [14]], "input groups stay unchanged");
  assert.deepEqual(groupThoughts(entries.slice(0, 2), [[10], [11]], 10), [[10, 11]]);
  assert.deepEqual(groupThoughts([], []), []);
});

test("output, messages and hidden entries separate thought groups", () => {
  for (const kind of ["tool", "agent", "user", "other"]) {
    const entries = [thought("First"), { kind, text: "Output" }, thought("Second")];
    assert.deepEqual(groupThoughts(entries, [[0], [1], [2]]), [[0], [1], [2]]);
    assert.deepEqual(groupThoughts(entries, [[0], [2]]), [[0], [2]], "folded output still separates thoughts");
  }
});

test("merged thoughts retain their text, formatted bodies and total duration", () => {
  const entries = [thought("First", 42000, "<p>First</p>"), thought("<Second>\nNext", 42000)];
  assert.deepEqual(mergedThought(entries, [10, 11], 10), {
    kind: "thought", text: "First\n\n<Second>\nNext",
    html: "<p>First</p>\n<p>&lt;Second&gt;<br>Next</p>", ms: 84000,
  });
});

test("a live or replayed thought leaves the combined duration unknown", () => {
  assert.equal(mergedThought([thought("First", 1000), thought("Still thinking")], [0, 1]).ms, null);
  assert.equal(mergedThought([thought("Replayed"), thought("Ended", 2000)], [0, 1]).ms, null);
});
