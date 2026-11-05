import test from "node:test";
import assert from "node:assert/strict";
import { escUp, type EscAt } from "../src/center/escup-state.ts";

const at = (over: Partial<EscAt>): EscAt => ({ page: "projects", picked: "acme/widget", tile: false, alone: false, ...over });

test("Esc climbs from an open tile to the project, the Projects page, then Home", () => {
  assert.deepEqual(escUp(at({ tile: true })), { to: "tile" });
  assert.deepEqual(escUp(at({})), { to: "projects", project: null });
  assert.deepEqual(escUp(at({ picked: null })), { to: "home" });
  assert.equal(escUp(at({ page: "home", picked: null })), null);
});

test("a project's own window stops at its page", () => {
  assert.equal(escUp(at({ alone: true })), null);
  assert.deepEqual(escUp(at({ alone: true, tile: true })), { to: "tile" });
});
