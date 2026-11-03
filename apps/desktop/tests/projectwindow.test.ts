import test from "node:test";
import assert from "node:assert/strict";
import { route, type ProjectScope } from "../src/projectwindow.ts";

const scope: ProjectScope = {
  project: "org/site",
  chatProject: s => ({ "chat-site": "org/site", "chat-other": "org/other" } as Record<string, string>)[s] ?? null,
  folders: [{ id: "build-box", folder: "/srv/site" }],
};
const agent = (machine_id: string, cwd: string) => ({ kind: "agent", machine_id, machine_name: machine_id, id: "s1", agent: "claude", title: null, cwd, status: "idle", external: false });

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
