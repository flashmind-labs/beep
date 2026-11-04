import test from "node:test";
import assert from "node:assert/strict";
import type { AgentModels } from "../src/agents/agentmodels.ts";
import { checkoutLabel, formProjects, harnessFor, keepEffort, knownWorktrees, makesBranch, modelRows, projectAt, startPlace } from "../src/agents/agentpick.ts";

const ORDER = ["claude", "codex", "gemini", "opencode", "pi", "flsh"];

test("the harness stays when the machine has it, else the machine's first by preference", () => {
  assert.equal(harnessFor("codex", ["claude", "codex"], ORDER), "codex");
  assert.equal(harnessFor("gemini", ["flsh", "codex"], ORDER), "codex");
  assert.equal(harnessFor(null, ["pi", "flsh"], ORDER), "pi");
  assert.equal(harnessFor("claude", [], ORDER), null);
});

test("a thinking level carries over only to a harness that takes it", () => {
  assert.equal(keepEffort("codex", "high"), "high");
  assert.equal(keepEffort("codex", "max"), null);
  assert.equal(keepEffort("flsh", "low"), null);
  assert.equal(keepEffort("claude", null), null);
});

const session = (machine_id: string, cwd: string, last_activity: number) => ({ machine_id, cwd, last_activity });

test("a checkout's worktrees come from where sessions ran, newest first", () => {
  const root = "/srv/project/widget";
  const sessions = [
    session("m1", `${root}/.worktrees/old`, 1),
    session("m1", `${root}/.worktrees/new/src`, 3),
    session("m1", `${root}/.worktrees/old`, 2),
    session("m2", `${root}/.worktrees/elsewhere`, 9),
    session("m1", root, 5),
  ];
  assert.deepEqual(knownWorktrees(sessions, "m1", `${root}/`), [`${root}/.worktrees/new`, `${root}/.worktrees/old`]);
  assert.deepEqual(knownWorktrees(sessions, null, root), []);
});

test("New branch and New worktree send a plan; the checkout and a worktree send a folder", () => {
  const cwd = "/srv/project/widget";
  assert.equal(checkoutLabel("branch"), "New branch");
  assert.equal(makesBranch("branch"), true);
  assert.equal(makesBranch("new"), true);
  assert.equal(makesBranch({ worktree: `${cwd}/.worktrees/fix-it` }), false);
  assert.deepEqual(startPlace("current", cwd, "main"), { cwd, checkout: null });
  assert.deepEqual(startPlace("branch", cwd, " "), { cwd, checkout: { new_branch: { base: null } } });
  assert.deepEqual(startPlace("new", cwd, null), { cwd, checkout: { new_worktree: { base: null } } });
  assert.deepEqual(startPlace({ worktree: `${cwd}/.worktrees/fix-it` }, cwd, "main"), { cwd: `${cwd}/.worktrees/fix-it`, checkout: null });
});

const ROWS = [
  { name: "acme/widget", machine_id: "a", folder: "/home/user/acme/widget" },
  { name: "acme/widget", machine_id: "b", folder: "/Users/user/Projects/widget" },
  { name: "acme/relay", machine_id: "a", folder: "/home/user/srv/relay" },
  { name: "acme/empty", machine_id: "a", folder: "" },
];

test("a folder picked is the project whose checkout it is", () => {
  const projects = formProjects(ROWS);
  assert.equal(projectAt(projects, "a", "/home/user/acme/widget/")?.name, "acme/widget");
  assert.equal(projectAt(projects, null, "~/srv/relay")?.name, "acme/relay");
  assert.equal(projectAt(projects, "b", "/home/user/srv/relay"), null, "not checked out on that machine");
  assert.equal(projectAt(projects, "a", ""), null);
});

test("the harness's own default entry folds into one Default row", () => {
  const claude: AgentModels = { pick: "listed", current: "default", models: [
    { id: "default", name: "Default (recommended)", description: "Opus 4.6 · Most capable for complex work" },
    { id: "sonnet", name: "Sonnet", description: null },
  ] };
  assert.deepEqual(modelRows(claude, "").rows.map(r => [r.value, r.label, r.note]),
    [["", "Default", "Opus 4.6"], ["sonnet", "Sonnet", "sonnet"]]);
});
