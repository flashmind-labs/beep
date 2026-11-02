import test from "node:test";
import assert from "node:assert/strict";
import { agentFor, duration, taskAction, taskSummary, prLabel, taskMeta, workerFolder, workerLine, barPlaceholder, chatStarted } from "../dist/project.js";

const MIN = 60000;

test("the project agent is found by its org/repo name", () => {
  const agents = [{ name: "acme/widget", chat_id: "c1" }, { name: "acme/app", chat_id: "c2" }];
  assert.equal(agentFor(agents, "acme/app").chat_id, "c2");
  assert.equal(agentFor(agents, "widget"), null);
  assert.equal(agentFor(null, "acme/app"), null);
});

test("durations read in minutes, hours and days", () => {
  assert.equal(duration(0, 30000), "under a minute");
  assert.equal(duration(0, 18 * MIN), "18 min");
  assert.equal(duration(0, 120 * MIN), "2 h");
  assert.equal(duration(0, 125 * MIN), "2 h 5 min");
  assert.equal(duration(0, 3 * 24 * 60 * MIN), "3 d");
  assert.equal(duration(10, 0), "under a minute");
});

test("open tasks offer Drop and finished ones Reopen", () => {
  for (const s of ["planned", "running", "blocked", "review"]) assert.equal(taskAction(s).change, "drop");
  for (const s of ["done", "dropped"]) assert.equal(taskAction(s).label, "Reopen");
});

test("the task count line names each state once, in order", () => {
  const tasks = [{ state: "done" }, { state: "running" }, { state: "review" }, { state: "running" }];
  assert.equal(taskSummary(tasks), "2 running · 1 in review · 1 done");
  assert.equal(taskSummary([]), "");
});

test("a task row links its issue and PR and names its worker and the task it waits for", () => {
  const first = { id: "t1", title: "Fix the build", state: "running" };
  const second = { id: "t2", title: "Settings tests", state: "planned", depends_on: "t1", issue_number: 46, issue_url: "https://example.com/acme/widget/issues/46", pr_url: "https://example.com/acme/widget/pull/41", branch: "beep/settings-tests", worker: "w1" };
  const workers = [{ id: "w1", agent: "claude", machine_name: "build-box" }];
  assert.deepEqual(taskMeta(second, [first, second], workers), [
    { text: "#46", url: second.issue_url },
    { text: "PR #41", url: second.pr_url },
    { text: "Claude Code on build-box" },
    { text: "beep/settings-tests", code: true },
    { text: "after Fix the build" },
  ]);
  assert.deepEqual(taskMeta(second, [{ ...first, state: "done" }, second], []).map(m => m.text), ["#46", "PR #41", "beep/settings-tests"]);
  assert.equal(prLabel("https://example.com/x/pulls/7"), "PR #7");
  assert.equal(prLabel("https://example.com/x"), "PR");
});

test("a worker's line names its agent, machine, worktree, state and time", () => {
  assert.equal(workerFolder("/srv/project/.worktrees/fix-build"), ".worktrees/fix-build");
  assert.equal(workerFolder("/srv/work/widget"), "work/widget");
  const w = { agent: "codex", machine_name: "build-box", folder: "/srv/project/.worktrees/bump", state: "ended", started_at: 0, ended_at: 42 * MIN };
  assert.equal(workerLine(w, 99 * MIN), "Codex · build-box · .worktrees/bump · ended · 42 min");
  assert.equal(workerLine({ ...w, state: "running", ended_at: null }, 18 * MIN), "Codex · build-box · .worktrees/bump · running · 18 min");
});

test("the prompt bar names the project and offers the chat once it has turns", () => {
  assert.equal(barPlaceholder("acme/widget"), "Ask the project agent about acme/widget");
  assert.equal(chatStarted(undefined), false);
  assert.equal(chatStarted([]), false);
  assert.equal(chatStarted([{ role: "scheduled", label: "Nightly" }, { role: "approval", action: "Create 3 tasks" }]), false);
  assert.equal(chatStarted([{ role: "user", text: "Plan the 0.4 release" }]), true);
  assert.equal(chatStarted([{ role: "assistant", html: "<p>Done</p>" }]), true);
});
