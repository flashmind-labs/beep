import test from "node:test";
import assert from "node:assert/strict";
import { updatedTo } from "../src/machines/machines-state.ts";

test("a restarting node is back once online on the new version or newer", () => {
  assert.ok(!updatedTo({ online: false, version: "0.1.24" }, "0.1.24"));
  assert.ok(!updatedTo({ online: true, version: "0.1.20" }, "0.1.24"));
  assert.ok(updatedTo({ online: true, version: "0.1.24" }, "0.1.24"));
  assert.ok(updatedTo({ online: true, version: "0.1.25" }, "0.1.24"));
});
