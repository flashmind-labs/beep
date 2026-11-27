import test from "node:test";
import assert from "node:assert/strict";
import { WindowHistory } from "../src/stores/center/window-router.ts";
import type { Location } from "../src/stores/center/state.ts";

const home: Location = { view: "home", arg: null };
const project: Location = { view: "projects", arg: { project: "example/project", tile: "running" } };
const draft: Location = { view: "pane", arg: { draft: { project: "example/project", machine: null } } };
const session: Location = { view: "pane", arg: { agent: { kind: "agent", machine_id: "box", machine_name: "Box", id: "session", agent: "codex", title: "Session", cwd: "/srv/project", status: "idle", external: false } } };

test("router history keeps live project state and clears forward entries", () => {
  const h = new WindowHistory(); h.start(home); h.visit(home, project); h.visit(project, draft);
  assert.deepEqual(h.back(draft), project);
  assert.deepEqual(h.back(project), home);
  assert.deepEqual(h.forward(home), project);
  h.visit(project, session);
  assert.equal(h.forward(session), null);
  assert.deepEqual(h.back(session), project);
});

test("starting a session replaces a draft and discards its forward branch", () => {
  const h = new WindowHistory(); h.start(home); h.visit(home, project); h.visit(project, draft); h.visit(draft, session);
  assert.deepEqual(h.back(session), draft);
  h.visit(draft, session, true);
  assert.equal(h.forward(session), null);
  assert.deepEqual(h.back(session), project);
  assert.deepEqual(h.forward(project), session);
});

test("a draft replacement retains the previous session in history", () => {
  const previous: Location = { view: "pane", arg: "settings" };
  const h = new WindowHistory(); h.start(previous); h.visit(previous, draft); h.visit(draft, session, true);
  assert.deepEqual(h.back(session), previous);
  assert.deepEqual(h.forward(previous), session);
});

test("reopening the same page adds no step and old history is bounded", () => {
  const h = new WindowHistory(); h.start(home); h.visit(home, home);
  assert.equal(h.back(home), null);
  let current: Location = home;
  for (let i = 0; i < 110; i++) { const next: Location = { view: "projects", arg: { project: `project-${i}`, tile: null } }; h.visit(current, next); current = next; }
  let count = 0, prior: Location | null;
  while ((prior = h.back(current))) { current = prior; count++; }
  assert.equal(count, 100);
});

test("router resolves the active typed screen path", async () => {
  const h = new WindowHistory();
  h.start(home);
  h.visit(home, project);
  await h.router.load();
  assert.equal(h.router.state.location.pathname, "/projects");
  assert.equal(h.router.state.matches.at(-1)?.routeId, "/projects");
  assert.deepEqual(h.history.location.state.page, project);
});
