import test from "node:test";
import assert from "node:assert/strict";
import type { AgentModels } from "../src/bar/agentmodels.ts";
import { baseFor, baseRows, checkoutFor, checkoutLabel, effortNote, effortsOf, folderBusy, folderPill, folderRows, formProjects, formTitle, harnessFor, keepEffort, knownWorktrees, machineRows, makesBranch, MAX_BASES, modelLabel, modelRows, pillDetail, placePopover, projectAt, projectFolders, railOf, startPlace, tildePath } from "../src/bar/agentpick.ts";

const ORDER = ["claude", "codex", "gemini", "opencode", "pi", "flsh"];

test("the rail shows every harness, enabled only where the machine has it", () => {
  const rail = railOf(["claude", "codex", "pi"], ["claude", "codex", "pi", "gemini", "opencode"]);
  assert.deepEqual(rail, [
    { agent: "claude", enabled: true },
    { agent: "codex", enabled: true },
    { agent: "gemini", enabled: false },
    { agent: "pi", enabled: true },
    { agent: "flsh", enabled: false },
    { agent: "opencode", enabled: false },
  ]);
});

test("the harness stays when the machine has it, else the machine's first by preference", () => {
  assert.equal(harnessFor("codex", ["claude", "codex"], ORDER), "codex");
  assert.equal(harnessFor("gemini", ["flsh", "codex"], ORDER), "codex");
  assert.equal(harnessFor(null, ["pi", "flsh"], ORDER), "pi");
  assert.equal(harnessFor("claude", [], ORDER), null);
});

test("each harness offers the thinking levels its CLI takes", () => {
  const values = (agent: string) => effortsOf(agent).map(e => e.value);
  assert.deepEqual(effortsOf("claude").map(e => e.label), ["Low", "Medium", "High", "Extra high", "Max"]);
  assert.deepEqual(values("claude"), ["low", "medium", "high", "xhigh", "max"]);
  assert.deepEqual(values("codex"), ["low", "medium", "high"]);
  assert.deepEqual(values("pi"), ["off", "minimal", "low", "medium", "high", "xhigh", "max"]);
  for (const agent of ["gemini", "flsh", "opencode"]) assert.deepEqual(values(agent), [], agent);
  assert.deepEqual(effortsOf(null), []);
  assert.equal(effortNote("flsh", "flsh"), "flsh sets thinking in its own config.");
  assert.match(effortNote("gemini", "Gemini CLI"), /^Gemini CLI has no thinking setting/);
});

test("a thinking level carries over only to a harness that takes it", () => {
  assert.equal(keepEffort("codex", "high"), "high");
  assert.equal(keepEffort("codex", "max"), null);
  assert.equal(keepEffort("flsh", "low"), null);
  assert.equal(keepEffort("claude", null), null);
});

test("the pill names the model and the thinking level picked", () => {
  assert.equal(pillDetail("Default", "claude", "medium"), "Default · Medium");
  assert.equal(pillDetail("Default", "claude", "xhigh"), "Default · Extra high");
  assert.equal(pillDetail("", "claude", null), "Default");
  assert.equal(pillDetail("openai:gpt-test", "flsh", "high"), "openai:gpt-test");
});

const listed: AgentModels = {
  pick: "listed",
  current: "fast",
  models: [
    { id: "fast", name: "Fast", description: null },
    { id: "deep-1", name: "Deep", description: "Slower" },
  ],
};

test("models list Default first and filter by name or id", () => {
  assert.deepEqual(modelRows(listed, "").rows.map(r => [r.value, r.label, r.note]),
    [["", "Default", "Fast"], ["fast", "Fast", "fast"], ["deep-1", "Deep", "deep-1"]]);
  assert.deepEqual(modelRows(listed, "DEEP").rows.map(r => r.value), ["deep-1"]);
  assert.equal(modelRows(listed, "zzz").note, "No model matches.");
  assert.equal(modelLabel(listed, "deep-1"), "Deep");
  assert.equal(modelLabel(listed, ""), "Default");
  assert.equal(modelLabel(null, "typed"), "typed");
});

test("a harness that names models takes what is typed, one that cannot pick shows Default", () => {
  const named: AgentModels = { pick: "named", current: "openai:gpt-test", models: [] };
  assert.deepEqual(modelRows(named, "anthropic:big").rows.map(r => r.value), ["", "anthropic:big"]);
  assert.equal(modelRows(named, "").note, "Type a model name to use it.");
  assert.deepEqual(modelRows({ pick: "fixed", current: null, models: [] }, "x").rows.map(r => r.value), [""]);
  assert.deepEqual(modelRows(null, "").rows.map(r => r.value), [""]);
  assert.match(modelRows(undefined, "").note, /^Asking/);
});

test("the Machine menu lists machines with agents, then those that cannot run one", () => {
  const hosts = [
    { machine_id: "b", machine_name: "mac-box", agents: ["gemini", "claude"] },
    { machine_id: "a", machine_name: "build-box", agents: ["pi", "codex", "claude"] },
  ];
  const known = [
    { id: "a", name: "build-box", online: true },
    { id: "w", name: "win-box", online: false },
    { id: "n", name: "nas-box", online: true },
  ];
  const rows = machineRows(hosts, known, a => ({ claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI", pi: "pi" })[a] ?? a, ORDER);
  assert.deepEqual(rows.map(r => [r.name, r.note, r.enabled, r.online]), [
    ["build-box", "Claude Code, Codex, pi", true, true],
    ["mac-box", "Claude Code, Gemini CLI", true, true],
    ["nas-box", "no coding agent", false, true],
    ["win-box", "offline", false, false],
  ]);
  assert.equal(machineRows(hosts, null, a => a, ORDER).length, 2);
});

const session = (machine_id: string, cwd: string, status = "working", external = false, last_activity = 0) =>
  ({ machine_id, cwd, status, external, last_activity });

test("Auto starts in the checkout unless one of our sessions runs there", () => {
  const sessions = [session("m1", "/srv/project/widget/"), session("m2", "/srv/project/other", "ended"), session("m3", "/srv/project/widget", "working", true)];
  assert.equal(folderBusy(sessions, "m1", "/srv/project/widget"), true);
  assert.equal(folderBusy(sessions, "m2", "/srv/project/other"), false, "an ended session");
  assert.equal(folderBusy(sessions, "m3", "/srv/project/widget"), false, "one started outside Beep");
  assert.equal(folderBusy(sessions, null, "/srv/project/widget"), false, "no machine yet");
  assert.equal(checkoutFor("auto", true), "new");
  assert.equal(checkoutFor("auto", false), "current");
  assert.equal(checkoutFor("current", true), "current");
  assert.deepEqual(checkoutFor({ worktree: "/w" }, false), { worktree: "/w" });
  assert.equal(checkoutLabel("auto", true), "Auto: New worktree");
  assert.equal(checkoutLabel("new", false), "New worktree");
  assert.equal(checkoutLabel({ worktree: "/srv/project/widget/.worktrees/fix-it" }, false), "Worktree fix-it");
});

test("a checkout's worktrees come from where sessions ran, newest first", () => {
  const root = "/srv/project/widget";
  const sessions = [
    session("m1", `${root}/.worktrees/old`, "ended", false, 1),
    session("m1", `${root}/.worktrees/new/src`, "ended", false, 3),
    session("m1", `${root}/.worktrees/old`, "ended", false, 2),
    session("m2", `${root}/.worktrees/elsewhere`, "ended", false, 9),
    session("m1", root, "ended", false, 5),
  ];
  assert.deepEqual(knownWorktrees(sessions, "m1", `${root}/`), [`${root}/.worktrees/new`, `${root}/.worktrees/old`]);
  assert.deepEqual(knownWorktrees(sessions, null, root), []);
});

test("New branch and New worktree send a plan; the checkout and a worktree send a folder", () => {
  const cwd = "/srv/project/widget";
  assert.equal(checkoutLabel("branch", false), "New branch");
  assert.equal(makesBranch("branch", false), true);
  assert.equal(makesBranch("auto", true), true, "Auto on a busy checkout makes a worktree");
  assert.equal(makesBranch("auto", false), false);
  assert.equal(makesBranch({ worktree: `${cwd}/.worktrees/fix-it` }, true), false);
  assert.deepEqual(startPlace("current", true, cwd, "main"), { cwd, checkout: null });
  assert.deepEqual(startPlace("auto", false, cwd, "main"), { cwd, checkout: null });
  assert.deepEqual(startPlace("auto", true, cwd, "main"), { cwd, checkout: { new_worktree: { base: "main" } } });
  assert.deepEqual(startPlace("branch", false, cwd, " "), { cwd, checkout: { new_branch: { base: null } } });
  assert.deepEqual(startPlace("new", false, cwd, null), { cwd, checkout: { new_worktree: { base: null } } });
  assert.deepEqual(startPlace({ worktree: `${cwd}/.worktrees/fix-it` }, false, cwd, "main"), { cwd: `${cwd}/.worktrees/fix-it`, checkout: null });
});

test("the Base menu lists the default branch first, then the newest", () => {
  const list = {
    default_branch: "main",
    branches: [
      { name: "old", local: true, committed_at: 1 },
      { name: "main", local: true, committed_at: 2 },
      { name: "beep/fix-it", local: false, committed_at: 3 },
    ],
  };
  assert.deepEqual(baseRows(list), ["main", "beep/fix-it", "old"]);
  assert.deepEqual(baseRows(list, "FIX"), ["beep/fix-it"]);
  assert.deepEqual(baseRows(null), []);
  const many = { default_branch: "main", branches: Array.from({ length: 50 }, (_, i) => ({ name: `b${i}`, local: true, committed_at: i })) };
  assert.equal(baseRows(many).length, MAX_BASES);
  assert.equal(baseFor(null, list), "main", "the default branch");
  assert.equal(baseFor("old", list), "old");
  assert.equal(baseFor(" ", null), null, "the checkout's HEAD");
});

test("folders under the home folder are written with ~", () => {
  assert.equal(tildePath("/srv/user", "/srv/user"), "~");
  assert.equal(tildePath("/srv/user/acme/widget/", "/srv/user"), "~/acme/widget");
  assert.equal(tildePath("/home/user/acme", null), "~/acme", "a /home folder without the home known");
  assert.equal(tildePath("/Users/user", undefined), "~");
  assert.equal(tildePath("/opt/work", "/srv/user"), "/opt/work");
  assert.equal(tildePath("  ", "/srv/user"), "");
});

const ROWS = [
  { name: "acme/widget", machine_id: "a", folder: "/home/user/acme/widget" },
  { name: "acme/widget", machine_id: "b", folder: "/Users/user/Projects/widget" },
  { name: "acme/relay", machine_id: "a", folder: "/home/user/srv/relay" },
  { name: "acme/empty", machine_id: "a", folder: "" },
];

test("projects gather their checkouts by name", () => {
  assert.deepEqual(formProjects(ROWS), [
    { name: "acme/relay", places: [{ machine: "a", cwd: "/home/user/srv/relay" }] },
    { name: "acme/widget", places: [{ machine: "a", cwd: "/home/user/acme/widget" }, { machine: "b", cwd: "/Users/user/Projects/widget" }] },
  ]);
  assert.deepEqual(formProjects(null), []);
});

test("outside a project the Folder menu lists Workspace, the machine's projects, then Type a folder", () => {
  const projects = formProjects(ROWS);
  const onB = projectFolders(projects, "b", "/Users/user");
  assert.deepEqual(onB, [{ project: "acme/widget", cwd: "/Users/user/Projects/widget", shown: "~/Projects/widget" }]);
  const rows = folderRows("", "/home/user", "/home/user", projectFolders(projects, "a", "/home/user"));
  assert.deepEqual(rows, [
    { kind: "workspace", shown: "~", on: true },
    { kind: "rule" },
    { kind: "project", project: "acme/relay", cwd: "/home/user/srv/relay", shown: "~/srv/relay", on: false },
    { kind: "project", project: "acme/widget", cwd: "/home/user/acme/widget", shown: "~/acme/widget", on: false },
    { kind: "rule" },
    { kind: "type" },
  ]);
  const picked = folderRows("/home/user/srv/relay/", null, null, projectFolders(projects, "a", null));
  assert.deepEqual(picked.filter(r => "on" in r && r.on).map(r => r.kind), ["project"]);
  assert.deepEqual(folderRows("", null, null, []).map(r => r.kind), ["workspace", "rule", "type"]);
  // On Auto each project shows at its folder under ~, which every machine reads as its own.
  assert.deepEqual(projectFolders(projects, null, null).map(f => f.cwd), ["~/srv/relay", "~/acme/widget"]);
});

test("the Folder pill reads Workspace ~ outside a project, else the folder", () => {
  assert.deepEqual(folderPill("", "/home/user", "/home/user"), { label: "Workspace", path: "~" });
  assert.deepEqual(folderPill("", null, null), { label: "Workspace", path: "~" });
  assert.deepEqual(folderPill("/home/user/acme/widget", "/home/user", "/home/user"), { label: "", path: "~/acme/widget" });
});

test("a folder picked is the project whose checkout it is", () => {
  const projects = formProjects(ROWS);
  assert.equal(projectAt(projects, "a", "/home/user/acme/widget/")?.name, "acme/widget");
  assert.equal(projectAt(projects, null, "~/srv/relay")?.name, "acme/relay");
  assert.equal(projectAt(projects, "b", "/home/user/srv/relay"), null, "not checked out on that machine");
  assert.equal(projectAt(projects, "a", ""), null);
});

test("the heading names the project's short name", () => {
  assert.deepEqual(formTitle("acme/widget"), { lead: "What should we build in", name: "widget" });
  assert.deepEqual(formTitle(null), { lead: "What should we build?", name: null });
});

test("the picker opens above its pill, else below, else cut to the roomier side, always inside the view", () => {
  const view = { width: 1000, height: 800 };
  const size = { width: 600, height: 340 };
  const pill = (top: number) => ({ top, left: 700, width: 200, height: 30 });
  assert.deepEqual(placePopover(pill(500), size, view), { top: 154, left: 300, width: 600, maxHeight: null }, "above, right edges lined up");
  assert.deepEqual(placePopover(pill(100), size, view), { top: 136, left: 300, width: 600, maxHeight: null }, "no room above: below");
  const short = { width: 1000, height: 500 };
  assert.deepEqual(placePopover(pill(300), size, short), { top: 8, left: 300, width: 600, maxHeight: 286 }, "neither fits: the roomier side, cut");
  assert.deepEqual(placePopover(pill(150), size, short), { top: 186, left: 300, width: 600, maxHeight: 306 }, "below when it has more room");
  const narrow = placePopover({ top: 300, left: 10, width: 100, height: 30 }, size, { width: 400, height: 800 });
  assert.equal(narrow.width, 384, "never wider than the view");
  assert.equal(narrow.left, 8, "kept inside on the left");
  const left = placePopover({ top: 500, left: 0, width: 60, height: 30 }, size, view);
  assert.equal(left.left, 8);
});
