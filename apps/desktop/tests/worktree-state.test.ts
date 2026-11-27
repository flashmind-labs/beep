import test from "node:test";
import assert from "node:assert/strict";
import { type Tree, type Entry, follow, followSteps, removedCheckout, reviewHeading, rootOf, treeAt, matchesTree } from "../src/stores/agentpane/worktree-state.ts";
import { homeShort } from "../src/stores/project/projects-state.ts";

const tree = (path: string, branch: string | null, own = false): Tree => ({ path, branch, head: "abc1234", own, created: false });
const main = tree("/srv/app", "main", true);
const fix = tree("/srv/app/.worktrees/fix", "fix");
const side = tree("/srv/app-side", "side");
const trees = [main, fix, side];
const tool = (text: string) => ({ kind: "tool", text });
const where = (entries: Entry[], from = "/srv/app") => follow(entries, trees, "/srv/app", from);

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
  const back = where([tool("Edit `/srv/app-side/src/a.rs` [completed]"), tool("Edit /srv/app/src/b.rs [completed]")]);
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

test("Flashmind follows the active row in a list and can switch back to the launch checkout", () => {
  const row = (path: string, current: boolean) => ({ path, current, available: true, bare: false });
  const create = tool(`worktree: Worktree create [completed]\n${JSON.stringify(row(fix.path, false))}`);
  const list = tool(`worktree: Worktree list [completed]\n${JSON.stringify([row(main.path, false), row(fix.path, true), row(side.path, false)])}`);
  const back = tool(`worktree: Worktree switch [completed]\n${JSON.stringify(row(main.path, true))}`);
  assert.equal(where([create]).hit, false);
  assert.equal(where([create, list]).last, fix.path);
  assert.equal(where([create, list, back]).last, main.path);
  assert.equal(where([tool(`worktree: Worktree status [completed]\n${JSON.stringify(row(`${fix.path}/missing`, true))}`)]).hit, false);
});

test("a running call is read again: only the entries before it are settled", () => {
  const got = where([
    tool("cd /srv/app-side [completed]"),
    tool("Edit `/srv/app/.worktrees/fix/a.rs` [in_progress]"),
    tool("Read /srv/app/b.rs [completed]"),
  ]);
  assert.equal(got.done, 1);
  assert.equal(got.kept, "/srv/app-side");
  assert.equal(got.last, "/srv/app/.worktrees/fix");
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

test("a folder that is gone gives the main checkout, a live one gives null", () => {
  const gone = (cwd: string, list: Tree[]) => removedCheckout(list, cwd);
  // The node lists the main checkout as own for a removed one.
  assert.equal(gone("/srv/app/.worktrees/fix", [main, side]), "/srv/app");
  assert.equal(gone("/srv/app/.worktrees/fix/crates/x/", [main]), "/srv/app");
  assert.equal(gone("/srv/app/.claude/worktrees/fix", [main]), "/srv/app");
  assert.equal(gone("/srv/app-gone", [main]), "/srv/app");
  // A live one inside the main checkout is its own tree.
  assert.equal(gone("/srv/app/.worktrees/fix", [{ ...main, own: false }, { ...fix, own: true }, side]), null);
  assert.equal(gone("/srv/app/.worktrees/fix/src", [{ ...main, own: false }, { ...fix, own: true }]), null);
  // The main checkout, or a folder in it.
  assert.equal(gone("/srv/app", trees), null);
  assert.equal(gone("/srv/app/crates/x", trees), null);
  assert.equal(gone("/srv/app/worktrees-notes", [main]), null);
  // No list, or no tree holding the folder.
  assert.equal(gone("/srv/app", []), null);
  assert.equal(gone("/srv/app", [{ ...main, own: false }]), null);
});

test("a review names its folder, with ~ for home, and says when it is a worktree", () => {
  assert.equal(reviewHeading(homeShort("/home/user/app/.worktrees/side"), true), "Review on worktree: ~/app/.worktrees/side");
  assert.equal(reviewHeading(homeShort("/Users/user/app/"), false), "Review on ~/app");
  assert.equal(reviewHeading(homeShort("/srv/app"), false), "Review on /srv/app");
});


test("read-only commands in the launch checkout preserve the active worktree", () => {
  for (const text of ["Read /srv/app/src/a.rs", "git -C /srv/app status", "cat /srv/app/README.md"]) {
    assert.equal(where([tool(`${text} [completed]`)], fix.path).last, fix.path);
  }
});

test("creation is a candidate until an edit or structured directory confirms usage", () => {
  const created = where([tool("git worktree add .worktrees/fix -b fix [completed]")]);
  assert.equal(created.last, main.path);
  assert.equal(created.hit, false);
  assert.equal(created.done, 1);
  const used = where([{ ...tool("exec_command [completed]"), tool: {
    name: "exec_command", input: JSON.stringify({ workdir: fix.path }), input_truncated: false,
    result: null, result_truncated: false,
  } }]);
  assert.equal(used.last, fix.path);
});

test("a tool run on another machine leaves the worktree as it is", () => {
  const remote = where([{ ...tool("remote_run [completed]"), tool: {
    name: "remote_run", input: JSON.stringify({ machine: "build-box", cwd: fix.path }), input_truncated: false,
    result: null, result_truncated: false,
  } }]);
  assert.equal(remote.last, main.path);
});

test("search matches branch names and full paths, ignoring case", () => {
  assert.equal(matchesTree(fix, " FIX "), true);
  assert.equal(matchesTree(fix, ".worktrees"), true);
  assert.equal(matchesTree(main, "fix"), false);
  assert.equal(matchesTree(main, ""), true);
});

test("creation followed by cd confirms usage before relative shell edits", () => {
  const got = where([
    tool("git worktree add -q -b fix .worktrees/fix main && cd .worktrees/fix && rg name src [completed]"),
    tool("python3 edit.py [completed]"),
  ]);
  assert.equal(got.last, fix.path);
  assert.equal(got.hit, true);
  assert.equal(got.unknown.length, 1);
});

test("creation of another candidate does not leave the current worktree", () => {
  const got = where([tool("git worktree add /srv/app-side -b side [completed]")], fix.path);
  assert.equal(got.last, fix.path);
  assert.equal(got.hit, false);
});

test("cd requests discovery even before a second worktree is listed", () => {
  const got = follow([tool("cd .worktrees/new && cargo test [completed]")], [main], main.path, main.path);
  assert.deepEqual(got.unknown, ["/srv/app/.worktrees/new"]);
  assert.equal(got.hit, false);
});

test("Claude's worktree folder also requests discovery", () => {
  const got = where([tool("cd .claude/worktrees/new [completed]")]);
  assert.deepEqual(got.unknown, ["/srv/app/.claude/worktrees/new"]);
});
