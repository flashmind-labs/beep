import test from "node:test";
import assert from "node:assert/strict";
import { agentName } from "../src/agents/running-state.ts";

test("Home names every harness by its label", () => {
  assert.equal(agentName("gemini"), "Gemini CLI");
  assert.equal(agentName("pi"), "pi");
  assert.equal(agentName("flsh"), "flsh");
  assert.equal(agentName(null), "Agent");
});
