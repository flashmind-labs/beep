import test from "node:test";
import assert from "node:assert/strict";
import { draftCwd, draftEsc } from "../src/agents/draft-state.ts";

test("a terminal of the empty view starts in the picked worktree, else the checkout", () => {
  assert.equal(draftCwd("/srv/project", "current"), "/srv/project");
  assert.equal(draftCwd("/srv/project", "new"), "/srv/project");
  assert.equal(draftCwd("/srv/project", { worktree: "/srv/project/.worktrees/fix" }), "/srv/project/.worktrees/fix");
  assert.equal(draftCwd("", "current"), "", "no checkout, no folder");
});

test("Esc in the empty view's input keeps a prompt and goes back when it is empty", () => {
  assert.equal(draftEsc(""), "project");
  assert.equal(draftEsc("  \n"), "project");
  assert.equal(draftEsc("fix the login test"), "stay");
});
