import test from "node:test";
import assert from "node:assert/strict";
import { newChatTarget } from "../src/stores/project/agent.ts";

test("a new agent starts in the project on screen or of the shown session, else the start form", () => {
  const projects = [
    { name: "acme/widget", machine_id: "box", folder: "/srv/widget" },
    { name: "acme/widget", machine_id: "mac", folder: "/w/widget" },
    { name: "acme/other", machine_id: "mac", folder: "/w/other" },
  ];
  const to = (at: Parameters<typeof newChatTarget>[0]) => newChatTarget(at, projects);
  // The project's page: this machine when it has a checkout, else the first.
  assert.deepEqual(to({ shown: "projects", picked: "acme/widget", mac: "mac" }), { project: "acme/widget", machine: "mac" });
  assert.deepEqual(to({ shown: "projects", picked: "acme/widget", mac: "laptop" }), { project: "acme/widget", machine: "box" });
  // A session of a project, in a worktree under its folder: on the session's machine.
  const agent = { machine_id: "box", cwd: "/srv/widget/.worktrees/fix" };
  assert.deepEqual(to({ shown: "pane", view: "agent", agent, mac: "mac" }), { project: "acme/widget", machine: "box" });
  assert.deepEqual(to({ shown: "pane", view: "agent", agent, mac: "laptop" }), { project: "acme/widget", machine: "box" });
  // A moved session's folder ends in a slash; a worktree outside the folder names its main checkout.
  assert.deepEqual(to({ shown: "pane", view: "agent", agent: { machine_id: "mac", cwd: "/w/widget/" } }), { project: "acme/widget", machine: "mac" });
  assert.deepEqual(to({ shown: "pane", view: "agent", agent: { machine_id: "mac", cwd: "/w/widget-fix", main: "/w/widget" } }), { project: "acme/widget", machine: "mac" });
  assert.equal(to({ shown: "pane", view: "agent", agent: { machine_id: "mac", cwd: "/w/widget-fix" } }), null);
  // A session of one project now working in another's checkout stays with the project it started in.
  assert.deepEqual(to({ shown: "pane", view: "agent", agent: { machine_id: "mac", cwd: "/w/other", main: "/w/widget" } }), { project: "acme/other", machine: "mac" });
  // A session outside every project, other pages and Home.
  assert.equal(to({ shown: "pane", view: "agent", agent: { machine_id: "box", cwd: "/tmp/x" } }), null);
  assert.equal(to({ shown: "projects", picked: null }), null);
  assert.equal(to({ shown: "home", picked: "acme/widget" }), null);
});
