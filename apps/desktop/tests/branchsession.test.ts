import test from "node:test";
import assert from "node:assert/strict";
import { branchSession } from "../src/review/branchsession-state.ts";

const s = (id: string, cwd: string, last_activity: number, machine_id = "m1") => ({ id, cwd, last_activity, machine_id });

test("the most recent session in the branch's worktree, running or not", () => {
  const list = [s("old", "/srv/w/.worktrees/a", 1), s("new", "/srv/w/.worktrees/a/", 9), s("other", "/srv/w/.worktrees/b", 20), s("away", "/srv/w/.worktrees/a", 30, "m2")];
  assert.equal(branchSession(list, "m1", "/srv/w/.worktrees/a")?.id, "new");
});

test("a branch with no worktree, or a session on the checkout that moved on, has none", () => {
  const list = [s("shared", "/srv/w", 5)];
  assert.equal(branchSession(list, "m1", null), undefined);
  assert.equal(branchSession(list, "m1", "/srv/w/.worktrees/a"), undefined);
  assert.equal(branchSession(list, "m1", "/srv/w")?.id, "shared");
});
