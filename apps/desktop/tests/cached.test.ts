import test from "node:test";
import assert from "node:assert/strict";
import { seed, type Seeded } from "../src/stores/center/cached.ts";
import type { HomeView } from "../src/stores/center/home-state.ts";

function empty(): Seeded {
  return { home: null, top: [], groups: undefined, sync: [] };
}

function home(): HomeView {
  return { machines: [], running: [], needs_you: [], projects: [] };
}

test("what the mesh already answered stays", () => {
  const state = empty();
  const answered = home();
  state.home = answered;
  state.top = ["mine"];
  assert.equal(seed(state, { home: home(), top: ["old"] }), false);
  assert.equal(state.home, answered);
  assert.deepEqual(state.top, ["mine"]);
});

test("a kept Home comes in with its pins, its machines connecting", () => {
  const state = empty();
  const kept: HomeView = { ...home(), machines: [{ id: "m", online: true, connecting: false } as unknown as NonNullable<HomeView["machines"]>[number]] };
  assert.equal(seed(state, { home: kept, top: ["pinned"] }), true);
  assert.deepEqual(state.top, ["pinned"]);
  assert.equal(state.home?.machines?.[0].online, false);
  assert.equal(state.home?.machines?.[0].connecting, true);
});
