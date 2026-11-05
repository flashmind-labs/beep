import test from "node:test";
import assert from "node:assert/strict";
import { newChatTarget } from "../src/project/agent.ts";

test("new chat starts in the project on screen or of the shown session, else general", () => {
  const agents = [{ name: "acme/widget", chat_id: "c1" }];
  const projects = [
    { name: "acme/widget", machine_id: "box", folder: "/srv/widget" },
    { name: "acme/widget", machine_id: "mac", folder: "/w/widget" },
    { name: "acme/other", machine_id: "mac", folder: "/w/other" },
  ];
  const to = (at: Parameters<typeof newChatTarget>[0]) => newChatTarget(at, agents, projects);
  // The project's page: this machine when it has a checkout, else the first.
  assert.deepEqual(to({ shown: "projects", picked: "acme/widget", mac: "mac" }), { project: "acme/widget", machine: "mac" });
  assert.deepEqual(to({ shown: "projects", picked: "acme/widget", mac: "laptop" }), { project: "acme/widget", machine: "box" });
  // A session of a project, in a worktree under its folder: this machine first, else the session's.
  const agent = { machine_id: "box", cwd: "/srv/widget/.worktrees/fix" };
  assert.deepEqual(to({ shown: "pane", view: "agent", agent, mac: "mac" }), { project: "acme/widget", machine: "mac" });
  assert.deepEqual(to({ shown: "pane", view: "agent", agent, mac: "laptop" }), { project: "acme/widget", machine: "box" });
  assert.deepEqual(to({ shown: "pane", view: "chat", session: "c1" }), { project: "acme/widget", machine: "box" });
  // A session outside every project, a general chat, other pages and Home.
  assert.equal(to({ shown: "pane", view: "agent", agent: { machine_id: "box", cwd: "/tmp/x" } }), null);
  assert.equal(to({ shown: "pane", view: "chat", session: "c9" }), null);
  assert.equal(to({ shown: "pane", view: "chat", session: null }), null);
  assert.equal(to({ shown: "projects", picked: null }), null);
  assert.equal(to({ shown: "home", picked: "acme/widget" }), null);
});
