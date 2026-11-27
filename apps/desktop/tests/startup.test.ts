import test from "node:test";
import assert from "node:assert/strict";
import { homeForDisplay, type HomeView } from "../src/stores/center/home-state.ts";
import { seed, type Seeded } from "../src/stores/center/cached.ts";

test("startup shows Home before any machine answers", () => {
  const state: Seeded = { home: null, top: [], groups: undefined, sync: [] };
  const initial = homeForDisplay(state.home);
  assert.deepEqual(initial.machines, []);
  assert.deepEqual(initial.running, []);
  assert.deepEqual(initial.needs_you, []);
  assert.equal(state.home, null);

  const cached: HomeView = { machines: [], running: [], needs_you: [], projects: [] };
  assert.equal(seed(state, { home: cached }), true);
  assert.deepEqual(homeForDisplay(state.home), cached);
});
