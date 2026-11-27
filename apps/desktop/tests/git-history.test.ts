import test from "node:test";
import assert from "node:assert/strict";
import { appendCommits, dayGroups, diffPaths, selectionKey, type GitCommit } from "../src/stores/review/history-state.ts";
const commit = (id: string, committed_at = 1): GitCommit => ({ id, parents: [], subject: id, author: "Test", committed_at });

test("history pagination preserves order and removes overlapping commits", () => {
  const first = [commit("a"), commit("b")];
  assert.deepEqual(appendCommits(first, [commit("b"), commit("c"), commit("c")]).map(c => c.id), ["a", "b", "c"]);
  assert.deepEqual(first.map(c => c.id), ["a", "b"]);
});
test("commits and worktrees keep distinct selection identities", () => {
  assert.equal(selectionKey({ kind: "commit", commit: commit("abc"), origin: "All history" }), "commit:abc");
  assert.equal(selectionKey({ kind: "working", worktree: { path: "/srv/project", head: "abc", branch: "main", main: true } }), "working:/srv/project");
});
test("file selection includes both sides of a rename without interpreting path syntax", () => {
  assert.deepEqual(diffPaths([{ path: "new [name].ts", old_path: "old.ts", status: "renamed" }], ["new [name].ts", "other.ts", "new [name].ts"]), ["old.ts", "new [name].ts", "other.ts"]);
});
test("history groups preserve commit order across merge dates", () => {
  const now = Date.UTC(2026, 10, 8, 12);
  const groups = dayGroups([commit("merge", now), commit("feature", now - 86400000), commit("main", now)]);
  assert.deepEqual(groups.map(([, rows]) => rows.map(c => c.id)), [["merge", "main"], ["feature"]]);
});
