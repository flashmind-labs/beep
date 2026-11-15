import test from "node:test";
import assert from "node:assert/strict";
import type { AgentSession } from "../src/agents/agents.ts";
import { assistantHarness, assistantHost, assistantRows, isAssistant, resolveAssistant } from "../src/agents/assistant-state.ts";

const HOME = "/home/user";

test("a session is the assistant by its flag, else when found outside Beep in the home folder off Windows", () => {
  const found = { assistant: false, cwd: HOME, parent: null, external: true };
  assert.equal(isAssistant({ assistant: true, cwd: "/srv/project", parent: null, external: false }, HOME, true), true);
  assert.equal(isAssistant(found, HOME, false), true);
  assert.equal(isAssistant(found, HOME, true), false);
  assert.equal(isAssistant({ ...found, parent: { machine_id: "build-box", id: "a1", key: "a1" } }, HOME, false), false);
  assert.equal(isAssistant({ ...found, cwd: `${HOME}/project` }, HOME, false), false);
  assert.equal(isAssistant(found, null, false), false);
});

test("a fresh start with no folder and no assistant flag is no assistant", () => {
  assert.equal(isAssistant({ assistant: false, cwd: HOME, parent: null, external: false }, HOME, false), false);
});

const session = (id: string, machine_id: string, cwd: string, status: string, last_activity: number, assistant = false, external = true): AgentSession => ({
  machine_id, machine_name: machine_id, id, agent: "codex", title: null, cwd, status, doing: null, started_at: 0, last_activity, external, question: null, assistant,
});

test("the assistant list splits running from ended, the latest first", () => {
  const machines = [
    { id: "build-box", env: { os: "linux", home: HOME } },
    { id: "win-box", env: { os: "windows", home: "C:\\Users\\user" } },
  ];
  const rows = assistantRows([
    session("old", "build-box", HOME, "idle", 10),
    session("new", "build-box", "/srv/x", "working", 30, true),
    session("done", "build-box", HOME, "ended", 20),
    session("project", "build-box", "/srv/project", "working", 40),
    session("windows", "win-box", "C:\\Users\\user", "idle", 50),
    session("unknown", "gone-box", HOME, "idle", 60),
    session("fresh", "build-box", HOME, "idle", 70, false, false),
  ], machines);
  assert.deepEqual(rows.running.map(s => s.id), ["new", "old"]);
  assert.deepEqual(rows.ended.map(s => s.id), ["done"]);
});

test("the assistant's harness is the one picked when offered, else the first offered", () => {
  const hosts = [{ machine_id: "build-box", machine_name: "build-box", agents: ["flsh", "codex"] }, { machine_id: "mac", machine_name: "mac", agents: ["pi"] }];
  assert.equal(assistantHarness("flsh", hosts), "flsh");
  assert.equal(assistantHarness(null, hosts), "codex");
  assert.equal(assistantHarness("gemini", hosts), "codex");
  assert.equal(assistantHarness(null, []), null);
  assert.equal(assistantHost(hosts, "mac", "pi")?.machine_id, "mac");
  assert.equal(assistantHost(hosts, "mac", "codex")?.machine_id, "build-box");
  assert.equal(assistantHost(hosts, null, "gemini"), null);
});

test("the assistant setting resolves to a harness a machine offers, keeping a model only with its harness", () => {
  const codex = [{ agents: ["codex"] }];
  assert.deepEqual(resolveAssistant({ agent: null, model: "model-a", effort: "max" }, codex), { agent: "codex", model: null, effort: null });
  assert.deepEqual(resolveAssistant({ agent: "flsh", model: "deepseek", effort: "high" }, codex), { agent: "codex", model: null, effort: "high" });
  assert.deepEqual(resolveAssistant({ agent: "codex", model: "gpt-x", effort: "low" }, codex), { agent: "codex", model: "gpt-x", effort: "low" });
  assert.deepEqual(resolveAssistant({ agent: null, model: null, effort: null }, []), { agent: null, model: null, effort: null });
});
