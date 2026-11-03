import test from "node:test";
import assert from "node:assert/strict";
import { agentColor, agentMeta, agentPrompt, agentsPlace } from "../src/customagents-state.ts";

test("the list is read on this Mac's checkout first, else the first one", () => {
  const rows = [
    { kind: "offline", id: "o", name: "old-box", folder: null },
    { kind: "here", id: "g", name: "gpu-box", folder: "/srv/project" },
    { kind: "here", id: "m", name: "MacBook", folder: "/Users/user/project", mac: true },
  ];
  assert.deepEqual(agentsPlace(rows), { id: "m", name: "MacBook", folder: "/Users/user/project" });
  assert.deepEqual(agentsPlace(rows.slice(0, 2)), { id: "g", name: "gpu-box", folder: "/srv/project" });
  assert.equal(agentsPlace(rows.slice(0, 1)), null);
});

test("Start asks for the agent by name", () => {
  assert.equal(agentPrompt("reviewer"), "Use the reviewer agent to ");
});

test("only named or hex colours reach a style", () => {
  assert.equal(agentColor("Blue"), "#3B82F6");
  assert.equal(agentColor("#a0b1c2"), "#a0b1c2");
  assert.equal(agentColor("red; background: url(x)"), null);
  assert.equal(agentColor(null), null);
});

test("the meta line says scope, model and tools", () => {
  assert.deepEqual(agentMeta({ scope: "project", model: "opus", tools: ["Read", "Grep"] }), ["project", "opus", "2 tools"]);
  assert.deepEqual(agentMeta({ scope: "user", model: null, tools: ["Read"] }), ["user", "Read"]);
});
