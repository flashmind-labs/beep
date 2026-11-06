import test from "node:test";
import assert from "node:assert/strict";
import { nodeBehind, offeredNode, updatedTo } from "../src/machines/machines-state.ts";

test("a restarting node is back once online on the new version or newer", () => {
  assert.ok(!updatedTo({ online: false, version: "0.1.24" }, "0.1.24"));
  assert.ok(!updatedTo({ online: true, version: "0.1.20" }, "0.1.24"));
  assert.ok(updatedTo({ online: true, version: "0.1.24" }, "0.1.24"));
  assert.ok(updatedTo({ online: true, version: "0.1.25" }, "0.1.24"));
});

test("the released beep-node is offered even when no updatable machine has it", () => {
  // Two nodes on 0.2.10; only the Mac's built-in node, which is not counted, has 0.2.12.
  const newest = offeredNode("0.2.12", ["0.2.10", "0.2.10"]);
  assert.equal(newest, "0.2.12");
  assert.ok(nodeBehind("0.2.10", newest));
  assert.ok(!nodeBehind("0.2.12", newest));
});

test("offline, the newest updatable machine is offered", () => {
  assert.equal(offeredNode(null, ["0.2.10", "0.2.11"]), "0.2.11");
  assert.equal(offeredNode(null, []), null);
  // A machine ahead of the release read earlier still counts.
  assert.equal(offeredNode("0.2.11", ["0.2.12"]), "0.2.12");
});
