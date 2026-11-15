import test from "node:test";
import assert from "node:assert/strict";
import type { AgentSession } from "../src/agents/agents.ts";
import { assistantHost, assistantRows, isAssistant, resolveAssistant } from "../src/agents/assistant-state.ts";

const HOME = "/home/user";

const OLD_NODE = { home: HOME, windows: false, marks: false };

test("a session is the assistant by its flag, else on an older node when top-level in the home folder off Windows", () => {
  const found = { assistant: false, cwd: HOME, parent: null };
  assert.equal(isAssistant({ assistant: true, cwd: "/srv/project", parent: null }, { ...OLD_NODE, windows: true }), true);
  assert.equal(isAssistant(found, OLD_NODE), true);
  assert.equal(isAssistant(found, { ...OLD_NODE, windows: true }), false);
  assert.equal(isAssistant({ ...found, parent: { machine_id: "build-box", id: "a1", key: "a1" } }, OLD_NODE), false);
  assert.equal(isAssistant({ ...found, cwd: `${HOME}/project` }, OLD_NODE), false);
  assert.equal(isAssistant(found, { ...OLD_NODE, home: null }), false);
});

test("a reviewer is never the assistant", () => {
  assert.equal(isAssistant({ assistant: false, cwd: HOME, parent: null, reviewer: true }, OLD_NODE), false);
  assert.equal(isAssistant({ assistant: true, cwd: HOME, parent: null, reviewer: true }, OLD_NODE), false);
});

test("a newer node's flag is the answer, as in WSL, where its home folder holds no assistant", () => {
  const newer = { ...OLD_NODE, marks: true };
  assert.equal(isAssistant({ assistant: false, cwd: HOME, parent: null }, newer), false);
  assert.equal(isAssistant({ assistant: true, cwd: HOME, parent: null }, newer), true);
});

const session = (id: string, machine_id: string, cwd: string, status: string, last_activity: number, assistant = false, external = true): AgentSession => ({
  machine_id, machine_name: machine_id, id, agent: "codex", title: null, cwd, status, doing: null, started_at: 0, last_activity, external, question: null, assistant,
});

test("the assistant list splits running from ended, the latest first", () => {
  const machines = [
    { id: "build-box", version: "0.7.12", env: { os: "linux", home: HOME } },
    { id: "win-box", version: "0.7.12", env: { os: "windows", home: "C:\\Users\\user" } },
    { id: "wsl-box", version: "0.7.13", env: { os: "linux", home: HOME } },
  ];
  const rows = assistantRows([
    session("old", "build-box", HOME, "idle", 10),
    session("new", "build-box", "/srv/x", "working", 30, true),
    session("done", "build-box", HOME, "ended", 20),
    session("project", "build-box", "/srv/project", "working", 40),
    session("windows", "win-box", "C:\\Users\\user", "idle", 50),
    session("unknown", "gone-box", HOME, "idle", 60),
    session("fresh", "build-box", HOME, "idle", 70, false, false),
    session("wsl", "wsl-box", HOME, "idle", 80),
    session("wsl-flagged", "wsl-box", "/srv/y", "idle", 90, true),
  ], machines);
  assert.deepEqual(rows.running.map(s => s.id), ["wsl-flagged", "fresh", "new", "old"]);
  assert.deepEqual(rows.ended.map(s => s.id), ["done"]);
});

test("the assistant's harness is the one picked when offered, else the first offered", () => {
  const hosts = [{ machine_id: "build-box", agents: ["flsh", "codex"], assistant: true }, { machine_id: "mac", agents: ["pi"], assistant: true }];
  const agent = (picked: string | null, on: typeof hosts) => resolveAssistant({ agent: picked, model: null, effort: null }, on).agent;
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
  assert.equal(resolveAssistant({ agent: "codex", model: "gpt-x", effort: null }, hosts).agent, "pi");
  assert.equal(assistantHost(hosts, "old-box", "codex"), null);
  assert.equal(resolveAssistant({ agent: null, model: null, effort: null }, [hosts[0]]).agent, null);
});

test("the assistant setting resolves to a harness a machine offers, keeping a model only with its harness", () => {
  const codex = [{ agents: ["codex"], assistant: true }];
  assert.deepEqual(resolveAssistant({ agent: null, model: "model-a", effort: "max" }, codex), { agent: "codex", model: null, effort: null });
  assert.deepEqual(resolveAssistant({ agent: "flsh", model: "deepseek", effort: "high" }, codex), { agent: "codex", model: null, effort: "high" });
  assert.deepEqual(resolveAssistant({ agent: "codex", model: "gpt-x", effort: "low" }, codex), { agent: "codex", model: "gpt-x", effort: "low" });
  assert.deepEqual(resolveAssistant({ agent: null, model: null, effort: null }, []), { agent: null, model: null, effort: null });
});
