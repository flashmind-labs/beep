import test from "node:test";
import assert from "node:assert/strict";
import { homeForDisplay, type HomeView } from "../src/center/home-state.ts";
import { seed, type Seeded } from "../src/center/cached.ts";

test("serverless startup shows Home before any machine answers", () => {
  const state: Seeded = { home: null, top: [], groups: undefined, sync: [], agents: undefined, overview: undefined };
  const initial = homeForDisplay(state.home, false);
  assert.deepEqual(initial?.machines, []);
  assert.deepEqual(initial?.running, []);
  assert.deepEqual(initial?.needs_you, []);
  assert.equal(state.home, null);

  const cached: HomeView = { machines: [], running: [], needs_you: [], coming_up: null, updates: null, projects: [] };
  assert.equal(seed(state, { home: cached }, new Set()), true);
  assert.deepEqual(homeForDisplay(state.home, false), cached);
});

test("a configured server keeps its initial connection state", () => {
  assert.equal(homeForDisplay(null, true), null);
});

test("live Home replaces the startup display with or without a server", () => {
  const live: HomeView = { machines: [], running: [], needs_you: [], coming_up: null, updates: null, projects: [] };
  assert.equal(homeForDisplay(live, false), live);
  assert.equal(homeForDisplay(live, true), live);
});
