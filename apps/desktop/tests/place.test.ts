import test from "node:test";
import assert from "node:assert/strict";
import { type WorkPlace, placeSub } from "../src/agents/place-state.ts";
import { type Tree, reportedRoot } from "../src/agentpane/worktree-state.ts";

const place = (over: Partial<WorkPlace>): WorkPlace => ({
  folder: "/srv/app/.worktrees/fix", main: "/srv/app", linked: true, branch: "fix", origin: "https://example.com/org/app.git", ...over,
});

test("a row in a linked worktree names the repository, the branch and the machine", () => {
  assert.equal(placeSub(place({}), "build-box"), "org/app · fix · build-box");
});

test("a detached worktree is named by its folder, and a missing remote by the main folder", () => {
  assert.equal(placeSub(place({ branch: null, origin: "" }), "build-box"), "app · fix · build-box");
});

test("a row in the main checkout or with no place says nothing", () => {
  assert.equal(placeSub(place({ linked: false, folder: "/srv/app", branch: "main" }), "build-box"), null);
  assert.equal(placeSub(null, "build-box"), null);
  assert.equal(placeSub(undefined, "build-box"), null);
});

const tree = (path: string, own = false): Tree => ({ path, branch: null, head: "abc1234", own });

test("the folder a machine reports is the worktree the pane reads", () => {
  const trees = [tree("/srv/app", true), tree("/srv/app/.worktrees/fix")];
  assert.equal(reportedRoot(trees, "/srv/app", "/srv/app/.worktrees/fix"), "/srv/app/.worktrees/fix");
  // The session's own worktree reads from the session's own folder.
  assert.equal(reportedRoot(trees, "/srv/app/crates/x", "/srv/app"), "/srv/app/crates/x");
});

test("a reported folder the list does not have yet is read as it is", () => {
  assert.equal(reportedRoot([], "/srv/app", "/srv/app/.worktrees/new/"), "/srv/app/.worktrees/new");
});
