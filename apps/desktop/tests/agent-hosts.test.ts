import { answers, calls } from "./chat-dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, ensureAgentHosts, type AgentHost } from "../src/agents/agents-state.ts";

const host: AgentHost = {
  machine_id: "box", machine_name: "Build box", agents: ["codex"], workspace: "/srv/project",
  home: null, capabilities: [], assistant: false, moves_chats: false,
};
const reads = () => calls.filter(call => call.cmd === "agent_hosts").length;

test("a failed first read of the hosts is not kept and the next call reads again", async t => {
  t.mock.method(console, "warn", () => {});
  AG.hosts = null;
  calls.length = 0;
  answers.agent_hosts = Promise.reject(new Error("offline"));

  assert.deepEqual(await ensureAgentHosts(), []);
  assert.equal(AG.hosts, null);

  answers.agent_hosts = [host];
  assert.deepEqual(await ensureAgentHosts(), [host]);
  assert.equal(reads(), 2);
  assert.deepEqual(AG.hosts, [host]);
});
