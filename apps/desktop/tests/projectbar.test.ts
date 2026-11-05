import test from "node:test";
import assert from "node:assert/strict";
import { barPlace, sessionIn, targetCheckout, targetFolders, targetPlan, forgeOf, reviewPrompt } from "../src/project/projectbar.ts";

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

const PR = { number: 41, title: "Login screen copy", url: "https://github.com/acme/widget/pull/41", mine: true };

test("forgeOf tells github.com from any other host", () => {
  assert.equal(forgeOf(PR.url), "github");
  assert.equal(forgeOf("https://git.example.org/acme/widget/pulls/41"), "forgejo");
});

test("reviewPrompt names the PR and how to post on GitHub", () => {
  const text = reviewPrompt(PR, "github");
  assert.match(text, /^Review PR #41: Login screen copy \(https:\/\/github\.com\/acme\/widget\/pull\/41\)\. /);
  assert.match(text, /never approve, merge or push/);
  assert.match(text, /gh api repos\/acme\/widget\/pulls\/41\/reviews --method POST --input <file>/);
  assert.doesNotMatch(text, /Someone else|not checked out/);
});

test("reviewPrompt on Forgejo uses teacli, and warns about others' and forked PRs", () => {
  const text = reviewPrompt({ ...PR, url: "https://git.example.org/acme/widget/pulls/41", mine: false }, "forgejo", true);
  assert.match(text, /teacli pr review submit acme\/widget 41 --state comment/);
  assert.match(text, /teacli pr diff acme\/widget 41/);
  assert.match(text, /Someone else wrote this PR: do not build or run its code\./);
  assert.match(reviewPrompt(PR, "github", true), /gh pr diff 41/);
});
