import test from "node:test";
import assert from "node:assert/strict";
import { PROJECT_TABS, agentInProject, mainTarget, opensNewWindow, projectFirst, route, startView, windowTitle, type ProjectScope } from "../src/projectwindow.ts";

test("cmd-click and middle-click open a project in a new window", () => {
  assert.equal(opensNewWindow({ button: 0, metaKey: true }), true);
  assert.equal(opensNewWindow({ button: 1, metaKey: false }), true);
  assert.equal(opensNewWindow({ button: 1, metaKey: true }), true);
});

test("a plain click or a right-click stays in the window", () => {
  assert.equal(opensNewWindow({ button: 0, metaKey: false }), false);
  assert.equal(opensNewWindow({ button: 2, metaKey: false }), false);
  assert.equal(opensNewWindow({ button: 2, metaKey: true }), false);
});

test("a project's window starts on that project", () => {
  assert.deepEqual(startView("org/site"), { view: "projects", arg: { project: "org/site" } });
});

test("the main window starts on what was left for it", () => {
  assert.equal(startView(null), null);
  assert.equal(startView(""), null);
});

test("a project's window is titled with its name", () => {
  assert.equal(windowTitle("org/site"), "org/site");
  assert.equal(windowTitle(null), "Beep");
});

const scope: ProjectScope = {
  project: "org/site",
  chatProject: s => ({ "chat-site": "org/site", "chat-other": "org/other" } as Record<string, string>)[s] ?? null,
  folders: [{ id: "build-box", folder: "/srv/site" }],
};
const agent = (machine_id: string, cwd: string) => ({ kind: "agent", machine_id, machine_name: machine_id, id: "s1", agent: "claude", title: null, cwd, status: "idle", external: false });

test("the main window keeps every target", () => {
  assert.equal(route(null, "home"), "here");
  assert.equal(route(null, "projects", { project: "org/other" }), "here");
  assert.equal(route(null, "pane", { chat: { session: null } }), "here");
});

test("a project window keeps its project and what opens from it", () => {
  assert.equal(route(scope, "projects", { project: "org/site" }), "here");
  assert.equal(route(scope, "pane", { agent: agent("build-box", "/srv/site") }), "here");
  assert.equal(route(scope, "pane", { agent: agent("build-box", "/srv/site/.worktrees/fix") }), "here");
  assert.equal(route(scope, "pane", { agentnew: { prompt: "" } }), "here");
  assert.equal(route(scope, "pane", { chat: { session: "chat-site" } }), "here");
});

test("a project window sends what is not its project to the main window", () => {
  assert.equal(route(scope, "home"), "main");
  assert.equal(route(scope, "terminals"), "main");
  assert.equal(route(scope, "research"), "main");
  assert.equal(route(scope, "projects", { project: "org/other" }), "main");
  assert.equal(route(scope, "projects", { project: null }), "main");
  assert.equal(route(scope, "pane", "settings"), "main");
  assert.equal(route(scope, "pane", "memories"), "main");
  assert.equal(route(scope, "pane", { chat: { session: null } }), "main");
  assert.equal(route(scope, "pane", { chat: { session: "chat-other" } }), "main");
  assert.equal(route(scope, "pane", { group: { id: "g1" } }), "main");
  assert.equal(route(scope, "pane", { agent: agent("build-box", "/srv/site-old") }), "main");
  assert.equal(route(scope, "pane", { agent: agent("gpu-box", "/srv/site") }), "main");
  assert.equal(route(scope, "pane", { agent: agent("build-box", "") }), "main");
});

test("an agent counts as the project's while its folders are unknown", () => {
  assert.equal(agentInProject({ machine_id: "build-box", cwd: "/anywhere" }, []), true);
});

test("the main window is told what to show", () => {
  assert.deepEqual(mainTarget("projects", { project: "org/other" }), { projects: { project: "org/other" } });
  assert.equal(mainTarget("home"), "home");
  assert.equal(mainTarget("terminals"), "terminals");
  assert.equal(mainTarget("pane", "settings"), "settings");
  assert.equal(mainTarget("pane", "agents"), "home");
  assert.deepEqual(mainTarget("pane", { chat: { session: "c1" } }), { chat: { session: "c1" } });
  assert.deepEqual(mainTarget("pane", { group: { id: "g1" } }), { group: { id: "g1" } });
  const sent = mainTarget("pane", { agent: { machine_id: "gpu-box", id: "s2" } });
  assert.deepEqual(sent, { agent: { kind: "agent", machine_id: "gpu-box", machine_name: "", id: "s2", agent: "", title: null, cwd: "", status: "idle", doing: null, started_at: 0, last_activity: 0, external: false } });
});

test("a project window's tabs start on the board", () => {
  assert.deepEqual(PROJECT_TABS[0], { key: null, label: "Overview" });
  assert.ok(PROJECT_TABS.some(t => t.key === "terminals"));
});

test("the palette lists the window's project first in each group", () => {
  const hit = (kind: string, id: string, title: string, detail = "", top = false) => ({ kind, id, title, detail, top });
  const hits = [
    hit("project", "org/other", "org/other", "", true),
    hit("project", "org/other2", "org/other2"),
    hit("project", "org/site", "org/site"),
    hit("chat", "c1", "Lunch plans"),
    hit("chat", "c2", "Deploy notes", "org/site"),
    hit("branch", "org/site fix", "fix"),
  ];
  const ids = projectFirst(hits, "org/site").map(h => h.id);
  assert.deepEqual(ids, ["org/other", "org/site", "org/other2", "c2", "c1", "org/site fix"]);
  assert.deepEqual(projectFirst(hits, null).map(h => h.id), hits.map(h => h.id));
});
