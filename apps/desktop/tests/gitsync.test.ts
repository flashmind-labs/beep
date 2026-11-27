import test from "node:test";
import assert from "node:assert/strict";
import { gitLine, moveTitle, projectBranch, withRow, type GitRow } from "../src/stores/project/gitsync-state.ts";

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

test("automatic Git conflicts warn while ordinary waiting does not", async () => {
  const { gitSyncWarning } = await import("../src/stores/project/gitsync-state.ts");

  for (const error of ["Git commits differ; histories diverged; merge the peer commits to resume sync", "Git has unmerged conflicts; resolve them to resume sync", "fatal: Not possible to fast-forward, aborting.", "Git commits differ between peers"]) {
    assert.equal(gitSyncWarning({ status: "failed", error }), "Git sync needs review");
    assert.equal(gitSyncWarning({ status: "failed", error: `waiting for Git: ${error}` }), "Git sync needs review");
    assert.equal(gitSyncWarning({ status: "in_sync", error }), null, "a stale error cannot warn");
    assert.equal(gitSyncWarning({ status: "offline", error }), null);
  }

  for (const error of [null, "Waiting for peer Git commits to arrive", "Git commits differ; waiting for peer commits to arrive", "Git commits differ; histories diverged or peer commits have not arrived", "Git commits differ between branches", "authentication failed", "disk full"]) {
    assert.equal(gitSyncWarning({ status: "failed", error }), null);
  }
});

test("repair draft includes both checkouts and bounded recent session references", async () => {
  const { syncRepairPrompt } = await import("../src/stores/project/gitsync-state.ts");
  const session = (id: string, machine_id: string, last_activity: number) => ({ id, machine_id, machine_name: machine_id, agent: "codex", title: id, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity, external: false, question: null, assistant: false, persona: null });
  const prompt = syncRepairPrompt({ project: "acme/widget", machine: "a", error: "histories diverged", checkouts: [{ id: "a", name: "build-box", folder: "/srv/project" }, { id: "b", name: "laptop", folder: "/srv/widget" }], git: [row({ machine_id: "a" })], sessions: [session("old", "a", 1), session("recent", "a", 3), session("newest", "a", 4), session("other-side", "b", 2), session("unrelated", "c", 5)] });
  for (const text of ["build-box", "laptop", "/srv/project", "/srv/widget", "origin/main", "newest", "recent", "other-side", "read_session_transcript", "next_before", "not verified commit authors", "without cherry-picking", "Do not create commits or push without my approval"]) assert.ok(prompt.includes(text), text);
  assert.ok(!prompt.includes("`old`"));
  assert.ok(!prompt.includes("unrelated"));
  assert.ok(prompt.indexOf("`newest`") < prompt.indexOf("`recent`"));
  assert.match(prompt, /^### laptop$/m);
});

test("local edits get a distinct warning without calling them a merge conflict", async () => {
  const { gitSyncWarning } = await import("../src/stores/project/gitsync-state.ts");

  for (const error of ["waiting for Git: Git commits differ; commit or stash staged, unstaged and untracked changes before syncing", "Your local changes would be overwritten by merge", "cannot pull with rebase: You have unstaged changes"]) {
    assert.equal(gitSyncWarning({ status: "failed", error }), "Local edits block Git sync");
  }
});

test("interrupted recovery raises a repair warning with the backup instructions", async () => {
  const { gitSyncWarning, syncRepairPrompt } = await import("../src/stores/project/gitsync-state.ts");
  const error = "waiting for Git: Git sync recovery needs review. Local work remains in refs/beep-recovery/example";
  assert.equal(gitSyncWarning({status:"failed", error}), "Git recovery needs review");
  const prompt = syncRepairPrompt({project:"acme/widget", machine:"build-box", error, checkouts:[], git:[], sessions:[]});
  assert.match(prompt, /pending.json/);
  assert.match(prompt, /Never apply a stash twice/);
});

test("the branch is this computer's, else the first one checked out", () => {
  const rows = [{ machine_id: "m1", branch: null }, { machine_id: "m2", branch: "main" }, { machine_id: "mac", branch: "signup" }];
  assert.equal(projectBranch(rows, "mac"), "signup");
  assert.equal(projectBranch(rows, "elsewhere"), "main");
  assert.equal(projectBranch(undefined, "mac"), null);
  assert.equal(projectBranch([{ machine_id: "m1", branch: null }], null), null);
});
