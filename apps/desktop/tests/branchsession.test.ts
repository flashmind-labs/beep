import test from "node:test";
import assert from "node:assert/strict";
import { branchSessions } from "../src/stores/review/branchsession-state.ts";

const s = (id: string, cwd: string, last_activity: number, machine_id = "m1") => ({ id, cwd, last_activity, machine_id });

test("the most recent session in the branch's worktree, running or not", () => {
  const list = [s("old", "/srv/w/.worktrees/a", 1), s("new", "/srv/w/.worktrees/a/", 9), s("other", "/srv/w/.worktrees/b", 20), s("away", "/srv/w/.worktrees/a", 30, "m2")];
  assert.equal(branchSessions(list, "m1", "/srv/w/.worktrees/a")[0]?.id, "new");
});

test("a branch with no worktree, or a session on the checkout that moved on, has none", () => {
  const list = [s("shared", "/srv/w", 5)];
  assert.deepEqual(branchSessions(list, "m1", null), []);
  assert.deepEqual(branchSessions(list, "m1", "/srv/w/.worktrees/a"), []);
  assert.equal(branchSessions(list, "m1", "/srv/w")[0]?.id, "shared");
});

test("sessions in subfolders belong to the closest worktree, on the same machine", () => {
  const paths = ["/srv/project", "/srv/project/.worktrees/login", "/srv/project/.worktrees/login/nested"];
  const list = [s("main", "/srv/project/src", 3), s("login", "/srv/project/.worktrees/login/src", 9), s("nested", paths[2], 20), s("prefix", "/srv/project/.worktrees/login-copy", 7), s("away", paths[1], 30, "m2")];
  assert.deepEqual(branchSessions(list, "m1", paths[1], paths).map(s => s.id), ["login"]);
  assert.deepEqual(branchSessions(list, "m1", paths[0], paths).map(s => s.id), ["prefix", "main"]);
  assert.deepEqual(branchSessions(list, "m1", null, paths), []);
});

test("a reported worktree takes precedence over where a session started", () => {
  const session = { ...s("moved", "/srv/project", 9), worktree: "/srv/project/.worktrees/login" };
  const paths = ["/srv/project", session.worktree];
  assert.equal(branchSessions([session], "m1", paths[1], paths)[0]?.id, "moved");
  assert.deepEqual(branchSessions([session], "m1", paths[0], paths), []);
  assert.equal(session.cwd, "/srv/project", "opening a session keeps its original folder");
});

test("Windows worktrees match separators and case without merging sibling paths", () => {
  const list = [s("new", "C:\\Work\\Project\\.worktrees\\Login\\src", 9), s("old", "c:/work/project/.worktrees/login/", 1), s("sibling", "c:/work/project/.worktrees/login-copy", 20)];
  assert.deepEqual(branchSessions(list, "m1", "c:/work/project/.worktrees/login").map(s => s.id), ["new", "old"]);
});
