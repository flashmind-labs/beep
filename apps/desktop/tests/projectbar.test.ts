import test from "node:test";
import assert from "node:assert/strict";
import { addHosts, type BarTarget, barPlace, blockedHosts, barPrompt, barReadiness, nextBarMachine, sessionIn, targetCheckout, targetFolders, targetPlan, forgeOf, reviewPrompt, fixedTitle, reviewTitled, taskSession, taskTitle } from "../src/stores/project/projectbar.ts";

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

test("machine switching cycles only through machines with an agent and a checkout", () => {
  assert.equal(nextBarMachine(PLACES, HOSTS, "b")?.machine_id, "m");
  assert.equal(nextBarMachine(PLACES, HOSTS, "m")?.machine_id, "b");
  assert.equal(nextBarMachine(PLACES.slice(0, 1), HOSTS, "m"), null);
  assert.equal(nextBarMachine([], HOSTS, ""), null);
});

test("the bar starts on the machine picked, else the first by name with a checkout and an agent", () => {
  assert.deepEqual(barPlace(PLACES, HOSTS, "m"), { machine_id: "m", machine_name: "mac-box", cwd: "/Users/user/acme/widget" });
  assert.deepEqual(barPlace(PLACES, HOSTS, ""), { machine_id: "b", machine_name: "build-box", cwd: "/home/user/acme/widget" }, "none picked yet");
  assert.deepEqual(barPlace(PLACES, HOSTS, "n"), { machine_id: "b", machine_name: "build-box", cwd: "/home/user/acme/widget" }, "no coding agent on the one picked");
  assert.deepEqual(barPlace(PLACES, HOSTS, "gone"), { machine_id: "b", machine_name: "build-box", cwd: "/home/user/acme/widget" }, "picked one no longer listed");
  assert.equal(barPlace(PLACES.slice(2), HOSTS, "m"), null, "no checkout on a machine with an agent");
  assert.equal(barPlace([], HOSTS, ""), null, "no checkout yet");
});

test("with no checkout an agent can run in, the bar offers the machines with an agent", () => {
  assert.deepEqual(addHosts(HOSTS).map(h => h.machine_name), ["build-box", "mac-box"]);
  assert.deepEqual(barReadiness(PLACES.slice(2), HOSTS, ""), { kind: "no checkout" });
  assert.equal(barPrompt("acme/widget", { kind: "no checkout" }), "Add acme/widget to a machine to start an agent");
  assert.equal(barPrompt("acme/widget", { kind: "ready" }), "Describe a task for a coding agent in acme/widget");
});

test("with no coding agent on any machine, the bar says so instead of doing nothing", () => {
  const bare = HOSTS.map(h => ({ ...h, agents: [] }));
  assert.deepEqual(barReadiness(PLACES, bare, ""), { kind: "no agent" });
  assert.deepEqual(barReadiness(PLACES, null, ""), { kind: "ready" }, "machines not read yet");
  assert.equal(barPrompt("acme/widget", { kind: "no agent" }), "No machine has a coding agent. Install one in Coding agents");
});

test("a checkout only on a machine whose agents cannot run names that machine and why", () => {
  const env = (tools: string[]) => ({ tools: tools.map(name => ({ name })) });
  const machines = [
    { id: "w", name: "Ubuntu on desk-pc", online: true, env: env(["claude"]), adapters: [{ agent: "claude", failed: "needs Node.js" }] },
    { id: "plain", name: "plain-box", online: true, env: env(["git"]) },
    { id: "away", name: "away-box", online: false, env: env(["codex"]) },
    { id: "b", name: "build-box", online: true, env: env(["codex"]) },
  ];
  const blocked = blockedHosts(machines, HOSTS);
  assert.deepEqual(blocked, [{ machine_id: "w", machine_name: "Ubuntu on desk-pc", problem: "Could not install the Claude Code adapter: needs Node.js" }]);
  const readiness = barReadiness([{ machine: "w", cwd: "/path/to/work" }], HOSTS, "", blocked);
  assert.deepEqual(readiness, { kind: "blocked", host: blocked[0] });
  assert.equal(barPrompt("local/wsltest", readiness), "local/wsltest is on Ubuntu on desk-pc, where no coding agent runs yet");
  assert.deepEqual(barReadiness(PLACES, HOSTS, "", blocked), { kind: "ready" }, "a machine that runs agents wins");
});

test("a target starts on its named or existing branch", () => {
  const issue = { kind: "issue" as const, number: 52, branch: " issue-52-crash ", base: "main", task: null };
  const pr = { kind: "pr" as const, number: 41, branch: "fix/login", task: null };
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

test("a review starts in the current checkout or points to its earlier session", () => {
  const target: BarTarget = { kind: "review", task: { kind: "review", number: 41 }, earlier: null };
  assert.equal(targetPlan(target), null);
  assert.deepEqual(targetCheckout(target), { label: "Current checkout", sub: "PR #41" });

  const earlier = { machine_id: "b", machine_name: "build-box", agent: "codex", id: "s1", cwd: "/w", status: "ended", external: false };
  assert.deepEqual(targetCheckout({ ...target, earlier }), { label: "Earlier review", sub: "PR #41" });
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
  assert.match(text, /gh pr diff 41 --repo acme\/widget/);
  assert.match(text, /git ls-remote <remote> refs\/pull\/41\/head/);
  assert.match(text, /git show <head-sha>:<path>/);
  assert.doesNotMatch(text, /Someone else/);
});

test("reviewPrompt on Forgejo uses teacli and keeps someone else's code read-only", () => {
  const text = reviewPrompt({ ...PR, url: "https://git.example.org/acme/widget/pulls/41", mine: false }, "forgejo");
  assert.match(text, /teacli pr review submit acme\/widget 41 --state comment/);
  assert.match(text, /teacli pr diff acme\/widget 41/);
  assert.match(text, /refs\/pull\/41\/head/);
  assert.match(text, /Someone else wrote this PR: do not build or run its code\./);
});

test("reviewPrompt reads the reviews already there; again, it reviews what changed in the same session", () => {
  const first = reviewPrompt(PR, "github");
  assert.match(first, /Read the reviews and comments already on it first/);
  assert.match(first, /gh api repos\/acme\/widget\/pulls\/41\/comments/);
  const again = reviewPrompt(PR, "github", true);
  assert.match(again, /^Review PR #41 again: /);
  assert.match(again, /which of your earlier comments are fixed/);
  assert.match(again, /git ls-remote <remote> refs\/pull\/41\/head/);
  assert.doesNotMatch(again, /git pull|git switch/);
});

test("a task's session is found by its fixed name, the newest on any machine", () => {
  const row = (id: string, title: string | null, last_activity: number) => ({ id, title, last_activity });
  const listed = [row("a", "Review #41", 10), row("b", "Review #41", 30), row("c", "Review #410", 50), row("d", "PR #41 login copy review", 60), row("e", null, 70), row("f", "Issue #41", 80), row("g", "Fix run #41 (CI)", 90)];
  const review = (number: number) => ({ kind: "review" as const, number });
  assert.equal(taskTitle(review(41)), "Review #41");
  assert.equal(taskSession(listed, review(41))?.id, "b");
  assert.equal(taskSession(listed, review(410))?.id, "c");
  assert.equal(taskSession(listed, review(42)), null);
  assert.equal(taskSession([], review(41)), null);
  assert.equal(taskSession(listed, { kind: "issue", number: 41 })?.id, "f");
  assert.equal(taskSession(listed, { kind: "run", workflow: "CI", number: 41 })?.id, "g");
  assert.equal(taskSession(listed, { kind: "run", workflow: "Release", number: 41 }), null);
});

test("only a task session's name is fixed, and only a review's keeps it from its branch's chat", () => {
  for (const title of ["Review #41", "Issue #41", "Fix run #41 (CI)"]) assert.ok(fixedTitle(title), title);
  for (const title of ["Review #41 again", "PR #41 login copy review", "Fix login bug #12", "Issue #41: crash", null]) assert.ok(!fixedTitle(title), String(title));
  assert.ok(reviewTitled("Review #41"));
  assert.ok(!reviewTitled("Issue #41"));
});
