import test from "node:test";
import assert from "node:assert/strict";
import { route, type WindowScope } from "../src/stores/project/projectwindow.ts";

const scope: WindowScope = {
  kind: "project",
  project: "org/site",
  folders: [{ id: "build-box", folder: "/srv/site" }],
};
const agent = (machine_id: string, cwd: string) => ({ kind: "agent", machine_id, machine_name: machine_id, id: "s1", agent: "claude", title: null, cwd, status: "idle", external: false });

test("a project window keeps its project and what opens from it", () => {
  assert.equal(route(scope, { view: "projects", arg: { project: "org/site" } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("build-box", "/srv/site") } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("build-box", "/srv/site/.worktrees/fix") } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { agentnew: { prompt: "", assistant: false } } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { draft: { project: "org/site", machine: null } } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { draft: { project: "org/other", machine: null } } }), "main");
});

test("a project window sends what is not its project to the main window", () => {
  assert.equal(route(scope, { view: "home", arg: null }), "main");
  assert.equal(route(scope, { view: "projects", arg: { project: "org/other" } }), "main");
  assert.equal(route(scope, { view: "projects", arg: { project: null } }), "main");
  assert.equal(route(scope, { view: "pane", arg: "settings" }), "main");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("build-box", "/srv/site-old") } }), "main");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("gpu-box", "/srv/site") } }), "main");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("build-box", "") } }), "main");
});
