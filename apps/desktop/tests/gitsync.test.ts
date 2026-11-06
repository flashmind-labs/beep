import test from "node:test";
import assert from "node:assert/strict";
import { gitLine, moveTitle, withRow, type GitRow } from "../src/project/gitsync-state.ts";

const row = (over: Partial<GitRow>): GitRow => ({ machine_id: "m1", branch: "main", upstream: "origin/main", ahead: 0, behind: 0, error: null, ...over });

test("a checkout with nothing to move has both buttons off", () => {
  const line = gitLine(row({}));
  assert.equal(line.text, "");
  assert.deepEqual([line.buttons, line.pull, line.push], [true, false, false]);
});

test("ahead and behind turn on both buttons, which say how far", () => {
  const line = gitLine(row({ ahead: 2, behind: 1 }));
  assert.equal(line.text, "");
  assert.deepEqual([line.pull, line.push], [true, true]);
  assert.equal(moveTitle("pull", 3), "Pull 3 commits");
  assert.equal(moveTitle("push", 1), "Push 1 commit");
  assert.equal(moveTitle("pull", 0), "Pull");
});

test("no upstream says not pushed, keeps Push on and Pull off", () => {
  const line = gitLine(row({ upstream: null }));
  assert.equal(line.text, "not pushed");
  assert.deepEqual([line.pull, line.push], [false, true]);
});

test("a detached HEAD and an unread checkout have no buttons", () => {
  assert.equal(gitLine(row({ branch: null })).buttons, false);
  const failed = gitLine(row({ error: "the project's folder is not there" }));
  assert.equal(failed.buttons, false);
  assert.equal(failed.error, true);
});

test("a move's answer replaces its machine's row", () => {
  const rows = [row({ machine_id: "a" }), row({ machine_id: "b", behind: 1 })];
  const next = withRow(rows, row({ machine_id: "b" }));
  assert.equal(next.length, 2);
  assert.equal(next.find(r => r.machine_id === "b")?.behind, 0);
  assert.equal(withRow(undefined, row({})).length, 1);
});
