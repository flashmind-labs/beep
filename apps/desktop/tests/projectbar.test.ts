import test from "node:test";
import assert from "node:assert/strict";
import { barCheckout, barPlace, barPrompt, sessionIn, targetCheckout, targetFolders, targetPlan } from "../src/projectbar.ts";
import { DEFAULT_CHECKOUT } from "../src/bar/agentpick.ts";

const HOSTS = [
  { machine_id: "m", machine_name: "mac-box", agents: ["claude"] },
  { machine_id: "b", machine_name: "build-box", agents: ["codex", "claude"] },
  { machine_id: "n", machine_name: "nas-box", agents: [] },
];
const PLACES = [
  { machine: "m", cwd: "/Users/user/acme/widget" },
  { machine: "b", cwd: "/home/user/acme/widget" },
  { machine: "n", cwd: "/srv/acme/widget" },
];

test("Auto starts on the first machine by name with a checkout and an agent", () => {
  assert.deepEqual(barPlace(PLACES, HOSTS, "auto"), { machine_id: "b", machine_name: "build-box", cwd: "/home/user/acme/widget", auto: true });
  assert.deepEqual(barPlace(PLACES, HOSTS, "m"), { machine_id: "m", machine_name: "mac-box", cwd: "/Users/user/acme/widget", auto: false });
  assert.equal(barPlace(PLACES, HOSTS, "n"), null, "no coding agent there");
  assert.equal(barPlace([], HOSTS, "auto"), null, "no checkout yet");
});

test("the field asks for a task, or for a machine and folder first", () => {
  assert.equal(barPrompt("acme/widget", true), "Describe a task for a coding agent in acme/widget");
  assert.equal(barPrompt("acme/widget", false), "Pick a machine and folder for acme/widget");
});

test("the Folder pill says what the agent works on", () => {
  assert.deepEqual(barCheckout("new", "main"), { label: "New worktree", sub: "from main" });
  assert.deepEqual(barCheckout("branch", null), { label: "New branch", sub: "from HEAD" });
  assert.deepEqual(barCheckout("current", "main"), { label: "Current checkout", sub: "as it is" });
  assert.deepEqual(barCheckout({ worktree: "/srv/acme/widget/.worktrees/fix-login" }, "main"), { label: "fix-login", sub: "worktree" });
  assert.deepEqual(barCheckout(DEFAULT_CHECKOUT, "main"), { label: "Current checkout", sub: "as it is" }, "a plain start makes no branch");
});

test("a target starts on its named or existing branch", () => {
  const issue = { kind: "issue" as const, number: 52, branch: " issue-52-crash ", base: "main" };
  const pr = { kind: "pr" as const, number: 41, branch: "fix/login" };
  assert.deepEqual(targetPlan(issue), { named_worktree: { base: "main", branch: "issue-52-crash" } });
  assert.deepEqual(targetPlan(pr), { branch_worktree: { branch: "fix/login" } });
  assert.deepEqual(targetCheckout(issue), { label: "New worktree", sub: "from main" });
  assert.deepEqual(targetCheckout(pr), { label: "Worktree fix/login", sub: "PR #41" });
  assert.deepEqual(targetFolders(pr, "/srv/widget/"), ["/srv/widget/.worktrees/fix-login"]);
  assert.deepEqual(targetFolders(pr, "/srv/widget", "/srv/other/"), ["/srv/widget/.worktrees/fix-login", "/srv/other"]);
});

test("a session already in the target's worktree is found, newest first", () => {
  const s = (id: string, machine_id: string, cwd: string, last_activity: number, external = false) => ({ id, machine_id, cwd, last_activity, external });
  const sessions = [
    s("old", "b", "/srv/widget/.worktrees/fix-login", 1),
    s("new", "b", "/srv/widget/.worktrees/fix-login/src", 5),
    s("theirs", "b", "/srv/widget/.worktrees/fix-login", 9, true),
    s("other", "m", "/srv/widget/.worktrees/fix-login", 9),
    s("near", "b", "/srv/widget/.worktrees/fix-login-2", 9),
  ];
  assert.equal(sessionIn(sessions, "b", ["/srv/widget/.worktrees/fix-login"])?.id, "new");
  assert.equal(sessionIn(sessions, "b", ["/srv/widget/.worktrees/nope"]), null);
});
