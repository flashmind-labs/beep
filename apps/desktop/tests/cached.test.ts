import test from "node:test";
import assert from "node:assert/strict";
import { seed, type Seeded } from "../src/center/cached.ts";
import type { HomeView } from "../src/center/home-state.ts";

function empty(): Seeded {
  return { home: null, top: [], groups: undefined, sync: [], agents: undefined, overview: undefined };
}

function home(updates: { id: number }[] | null = null): HomeView {
  return { machines: [], running: [], needs_you: [], coming_up: null, updates, projects: [] } as unknown as HomeView;
}

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
