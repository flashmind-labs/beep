import test from "node:test";
import assert from "node:assert/strict";
import { type Tree, follow, followSteps, reviewIn, rootOf, treeAt } from "../src/agentpane/worktree-state.ts";

const tree = (path: string, branch: string | null, own = false): Tree => ({ path, branch, head: "abc1234", own });
const main = tree("/srv/app", "main", true);
const fix = tree("/srv/app/.worktrees/fix", "fix");
const side = tree("/srv/app-side", "side");
const trees = [main, fix, side];
const tool = (text: string) => ({ kind: "tool", text });
const where = (entries: { kind: string; text: string }[], from = "/srv/app") => follow(entries, trees, "/srv/app", from);

test("a path is in a worktree at a folder boundary", () => {
  assert.equal(treeAt(trees, "/srv/app", "/srv/app-side/src/a.rs"), side);
  assert.equal(treeAt(trees, "/srv/app", "/srv/app-sidecar/a.rs"), null);
  assert.equal(treeAt(trees, "/srv/app", "/srv/app-side"), side);
});

test("a worktree inside the main checkout wins over it", () => {
  assert.equal(treeAt(trees, "/srv/app", "/srv/app/.worktrees/fix/src/a.rs"), fix);
  assert.equal(treeAt(trees, "/srv/app", "/srv/app/src/a.rs"), main);
});

test("a session in a subfolder of its worktree matches the whole worktree", () => {
  assert.equal(treeAt(trees, "/srv/app/crates/x", "/srv/app/README.md"), main);
  assert.equal(rootOf(main, "/srv/app/crates/x/"), "/srv/app/crates/x");
  assert.equal(rootOf(side, "/srv/app/crates/x"), "/srv/app-side");
});

test("a path in no worktree changes nothing", () => {
  const got = where([tool("Read /etc/hosts [completed]"), tool("Edit `/home/u/notes.md` [completed]")], "/srv/app-side");
  assert.equal(got.last, "/srv/app-side");
  assert.equal(got.hit, false);
});

test("a tool title naming a path in another worktree moves the agent there, and back", () => {
  const into = where([tool("Edit `/srv/app-side/src/a.rs` [completed]")]);
  assert.equal(into.last, "/srv/app-side");
  assert.equal(into.hit, true);
  const back = where([tool("Edit `/srv/app-side/src/a.rs` [completed]"), tool("Read /srv/app/src/b.rs [completed]")]);
  assert.equal(back.last, "/srv/app");
});

test("a diff in the output names a path outside the session's folder in full", () => {
  const got = where([tool("Edit [completed]\n[diff] /srv/app-side/src/a.rs +1 -1\n-a\n+b")]);
  assert.equal(got.last, "/srv/app-side");
});

test("a diff inside the session's folder is relative to it, and may reach a nested worktree", () => {
  const got = where([tool("Edit [completed]\n[diff] .worktrees/fix/src/a.rs +1 -0\n+b")]);
  assert.equal(got.last, "/srv/app/.worktrees/fix");
});

test("a shell command's text moves the agent: cd, git -C and plain paths", () => {
  assert.equal(where([tool("cd /srv/app-side && cargo test [completed]")]).last, "/srv/app-side");
  assert.equal(where([tool("git -C /srv/app-side status [completed]")]).last, "/srv/app-side");
  assert.equal(where([tool("cd .worktrees/fix && ls [completed]")]).last, "/srv/app/.worktrees/fix");
  assert.equal(where([tool("cat src/a.rs [completed]")]).last, "/srv/app");
});

test("a relative path is taken from the last worktree seen", () => {
  const got = where([tool("cd /srv/app-side [completed]"), tool("cd ../app/.worktrees/fix [completed]")]);
  assert.equal(got.last, "/srv/app/.worktrees/fix");
  const stays = where([tool("cd /srv/app/.worktrees/fix [completed]"), tool("Read src/a.rs [completed]")]);
  assert.equal(stays.last, "/srv/app/.worktrees/fix");
});

test("a failed call and plain messages change nothing", () => {
  const got = where([
    tool("cd /srv/app-side && make [failed]"),
    { kind: "agent", text: "Working in /srv/app-side now" },
  ]);
  assert.equal(got.last, "/srv/app");
});

test("EnterWorktree and ExitWorktree move the agent by the path they report", () => {
  const entered = where([tool("EnterWorktree [completed]\nCreated worktree at /srv/app-side on branch side")]);
  assert.equal(entered.last, "/srv/app-side");
  const left = where([
    tool("EnterWorktree [completed]\nCreated worktree at /srv/app-side on branch side"),
    tool("ExitWorktree [completed]\nKept the worktree"),
  ]);
  assert.equal(left.last, "/srv/app");
  assert.equal(left.hit, true);
});

test("a running call is read again: only the entries before it are settled", () => {
  const got = where([
    tool("cd /srv/app-side [completed]"),
    tool("Edit `/srv/app/.worktrees/fix/a.rs` [in_progress]"),
    tool("Read /srv/app/b.rs [completed]"),
  ]);
  assert.equal(got.done, 1);
  assert.equal(got.kept, "/srv/app-side");
  assert.equal(got.last, "/srv/app");
});

test("a git worktree command or a worktree folder the list lacks asks for the list again", () => {
  const got = where([tool("git worktree add .worktrees/new -b new [completed]")]);
  assert.equal(got.unknown.length >= 1, true);
  const folder = where([tool("cd /srv/app/.worktrees/new [completed]")]);
  assert.deepEqual(folder.unknown, ["/srv/app/.worktrees/new"]);
  const known = where([tool("cd /srv/app/.worktrees/fix [completed]")]);
  assert.deepEqual(known.unknown, []);
});

test("with one worktree there is nothing to follow", () => {
  const got = follow([tool("cd /srv/app-side [completed]")], [main], "/srv/app", "/srv/app");
  assert.equal(got.last, "/srv/app");
  assert.equal(got.hit, false);
});

test("a subagent's steps give its worktree", () => {
  const got = followSteps(["cd /srv/app-side", "cargo test"], trees, "/srv/app");
  assert.equal(got.last, "/srv/app-side");
  assert.equal(got.hit, true);
  assert.equal(followSteps(["cargo test"], trees, "/srv/app").hit, false);
});

test("a review of another worktree says which", () => {
  assert.equal(reviewIn("Review of your changes:\n\nx", "/srv/app", "/srv/app/"), "Review of your changes:\n\nx");
  assert.equal(reviewIn("Review of your changes:\n\nx", "/srv/app-side", "/srv/app"), "In /srv/app-side: Review of your changes:\n\nx");
});
