import test from "node:test";
import assert from "node:assert/strict";
import { seed, type Seeded } from "../src/cached.ts";
import type { HomeView } from "../src/home.ts";

function empty(): Seeded {
  return { home: null, top: [], groups: undefined, sync: [], agents: undefined, overview: undefined };
}

function home(updates: { id: number }[] | null = null): HomeView {
  return { machines: [], running: [], needs_you: [], coming_up: null, updates, projects: [] } as unknown as HomeView;
}

test("nothing kept leaves the window waiting for the server", () => {
  const state = empty();
  assert.equal(seed(state, null, new Set()), false);
  assert.equal(seed(state, {}, new Set()), false);
  assert.deepEqual(state, empty());
});

test("kept views fill a window that has nothing yet", () => {
  const state = empty();
  const groups = [{ id: "g", name: "Work", paths: [] }] as unknown as Seeded["groups"];
  assert.equal(seed(state, { home: home(), top: ["org/repo"], groups, sync: [], agents: null, overview: [] }, new Set()), true);
  assert.ok(state.home);
  assert.deepEqual(state.top, ["org/repo"]);
  assert.equal(state.groups, groups);
  assert.equal(state.agents, null);
  assert.deepEqual(state.overview, []);
});

test("what the server already answered stays", () => {
  const state = empty();
  const answered = home();
  state.home = answered;
  state.top = ["mine"];
  state.agents = [];
  assert.equal(seed(state, { home: home(), top: ["old"], agents: null, overview: null }, new Set()), false);
  assert.equal(state.home, answered);
  assert.deepEqual(state.top, ["mine"]);
  assert.deepEqual(state.agents, []);
  assert.equal(state.overview, null);
});

test("a dismissed update stays off a kept Home", () => {
  const state = empty();
  const dismissed = new Set([2]);
  seed(state, { home: home([{ id: 1 }, { id: 2 }]) }, dismissed);
  assert.deepEqual(state.home?.updates?.map(u => u.id), [1]);
  assert.ok(dismissed.has(2), "the server has not said it is gone");
});
