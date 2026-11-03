import test from "node:test";
import assert from "node:assert/strict";
import { agentColor } from "../src/customagents-state.ts";

test("only named or hex colours reach a style", () => {
  assert.equal(agentColor("Blue"), "#3B82F6");
  assert.equal(agentColor("#a0b1c2"), "#a0b1c2");
  assert.equal(agentColor("red; background: url(x)"), null);
  assert.equal(agentColor(null), null);
});
