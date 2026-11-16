import test from "node:test";
import assert from "node:assert/strict";
import type { AgentSession } from "../src/agents/agents.ts";
import { assistantHost, assistantRows, isAssistant, resolveAssistant } from "../src/agents/assistant-state.ts";

const HOME = "/home/user";

test("a session is the assistant only by its flag, never as a reviewer", () => {
  assert.equal(isAssistant({ assistant: true }), true);
  assert.equal(isAssistant({ assistant: false }), false);
  assert.equal(isAssistant({ assistant: true, reviewer: true }), false);
});

const session = (id: string, cwd: string, status: string, last_activity: number, assistant = false): AgentSession => ({
  machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: null, cwd, status, doing: null, started_at: 0, last_activity, external: false, question: null, assistant,
});

test("the assistant list splits running from ended, the latest first, and leaves out home folder sessions without the flag", () => {
  const rows = assistantRows([
    session("home", HOME, "idle", 10),
    session("new", "/srv/x", "working", 30, true),
    session("done", HOME, "ended", 20, true),
    session("project", "/srv/project", "working", 40),
    session("latest", HOME, "idle", 50, true),
  ]);
  assert.deepEqual(rows.running.map(s => s.id), ["latest", "new"]);
  assert.deepEqual(rows.ended.map(s => s.id), ["done"]);
});

test("the assistant's harness is the one picked when offered, else the first offered", () => {
  const hosts = [{ machine_id: "build-box", agents: ["flsh", "codex"], assistant: true }, { machine_id: "mac", agents: ["pi"], assistant: true }];
  const agent = (picked: string | null, on: typeof hosts) => resolveAssistant({ agent: picked, model: null, effort: null, fast: null }, on).agent;
  assert.equal(agent("flsh", hosts), "flsh");
  assert.equal(agent(null, hosts), "codex");
  assert.equal(agent("gemini", hosts), "codex");
  assert.equal(agent(null, []), null);
  assert.equal(assistantHost(hosts, "mac", "pi")?.machine_id, "mac");
  assert.equal(assistantHost(hosts, "mac", "codex")?.machine_id, "build-box");
  assert.equal(assistantHost(hosts, null, "gemini"), null);
});

test("only machines whose node runs the assistant count for its harness and machine", () => {
  const hosts = [{ machine_id: "old-box", agents: ["codex"], assistant: false }, { machine_id: "mac", agents: ["pi"], assistant: true }];
  assert.equal(resolveAssistant({ agent: "codex", model: "gpt-x", effort: null, fast: null }, hosts).agent, "pi");
  assert.equal(assistantHost(hosts, "old-box", "codex"), null);
  assert.equal(resolveAssistant({ agent: null, model: null, effort: null, fast: null }, [hosts[0]]).agent, null);
});

test("the assistant setting resolves to a harness a machine offers, keeping a model only with its harness", () => {
  const codex = [{ agents: ["codex"], assistant: true }];
  assert.deepEqual(resolveAssistant({ agent: null, model: "model-a", effort: "max", fast: null }, codex), { agent: "codex", model: null, effort: "max", fast: null });
  assert.deepEqual(resolveAssistant({ agent: "flsh", model: "deepseek", effort: "high", fast: null }, codex), { agent: "codex", model: null, effort: "high", fast: null });
  assert.deepEqual(resolveAssistant({ agent: "codex", model: "gpt-x", effort: "low", fast: null }, codex), { agent: "codex", model: "gpt-x", effort: "low", fast: null });
  assert.deepEqual(resolveAssistant({ agent: null, model: null, effort: null, fast: null }, []), { agent: null, model: null, effort: null, fast: null });
});
