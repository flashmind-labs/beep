import test from "node:test";
import assert from "node:assert/strict";
import type { AgentSession } from "../src/stores/agents/agents-state.ts";
import { assistantHost, assistantRows, isAssistant, resolveAssistant } from "../src/stores/agents/assistant-state.ts";

const HOME = "/home/user";

test("a session is the assistant only by its flag, never as a reviewer", () => {
  assert.equal(isAssistant({ assistant: true }), true);
  assert.equal(isAssistant({ assistant: false }), false);
  assert.equal(isAssistant({ assistant: true, reviewer: true }), false);
});

const session = (id: string, cwd: string, status: string, started_at: number, last_activity: number, assistant = false): AgentSession => ({
  machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: null, cwd, status, doing: null, started_at, last_activity, external: false, question: null, assistant, persona: null,
});

test("Assistant lists open and ended chats by creation time and leaves out other sessions", () => {
  const rows = assistantRows([
    session("home", HOME, "idle", 1, 10),
    session("older", "/srv/x", "working", 2, 60, true),
    session("done-older", HOME, "ended", 1, 70, true),
    session("project", "/srv/project", "working", 4, 40),
    session("newer-b", HOME, "idle", 3, 30, true),
    session("newer-a", HOME, "idle", 3, 50, true),
    session("done-newer", HOME, "ended", 4, 20, true),
  ]);
  assert.deepEqual(rows.running.map(s => s.id), ["newer-a", "newer-b", "older"]);
  assert.deepEqual(rows.ended.map(s => s.id), ["done-newer", "done-older"]);
});

test("the assistant's harness is the one picked when offered, else the first offered", () => {
  const hosts = [{ machine_id: "build-box", agents: ["flsh", "codex"], assistant: true }, { machine_id: "mac", agents: ["pi"], assistant: true }];
  const agent = (picked: string | null, on: typeof hosts) => resolveAssistant({ agent: picked, model: null, effort: null, fast: null }, on).agent;
  assert.equal(agent("flsh", hosts), "flsh");
  assert.equal(agent(null, hosts), "codex");
  assert.equal(agent("agy", hosts), "codex");
  assert.equal(agent(null, []), null);
  assert.equal(assistantHost(hosts, "mac", "pi")?.machine_id, "mac");
  assert.equal(assistantHost(hosts, "mac", "codex")?.machine_id, "build-box");
  assert.equal(assistantHost(hosts, null, "agy"), null);
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
