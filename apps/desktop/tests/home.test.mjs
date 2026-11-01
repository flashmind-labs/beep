import test from "node:test";
import assert from "node:assert/strict";
import { homeColumns, homeRows, machineCaps, machineNow, machineOs, needsLine, runningLine } from "../dist/home.js";

const machine = (over = {}) => ({
  online: true, agents: 0, capabilities: [], gpus: [], apps: [], env: null, ...over,
});

test("a named card replaces the plain GPU and nothing shows twice", () => {
  const m = machine({ capabilities: ["gpu", "docker"], gpus: [{ model: "RTX 4090" }], apps: [{ name: "Docker" }, { name: "Blender" }] });
  assert.deepEqual(machineCaps(m), ["Docker", "RTX 4090", "Blender"]);
  assert.deepEqual(machineCaps(machine({ capabilities: ["gpu", "xcode"] })), ["GPU", "Xcode"]);
});

test("Now says offline, working or idle", () => {
  assert.equal(machineNow(machine({ online: false, agents: 2 })), "offline");
  assert.equal(machineNow(machine({ agents: 1 })), "working");
  assert.equal(machineNow(machine()), "idle");
});

test("the OS line prefers the version", () => {
  assert.equal(machineOs(machine({ env: { os: "linux", os_version: "Ubuntu 24.04" } })), "Ubuntu 24.04");
  assert.equal(machineOs(machine({ env: { os: "macos", os_version: "" } })), "macos");
  assert.equal(machineOs(machine()), "");
});

test("a running agent shows its title, then project and machine", () => {
  const r = { kind: "agent", agent: "claude", title: "Fix the build", cwd: "/srv/project/widget", machine_name: "build-box" };
  assert.deepEqual(runningLine(r), { title: "Fix the build", sub: "widget · build-box" });
  assert.deepEqual(runningLine({ ...r, title: null, cwd: "" }), { title: "Claude Code", sub: "Claude Code · build-box" });
});

test("a chat approval allows from the row and names no device", () => {
  const n = { kind: "chat", session_title: "Deposit", pending: { approval: { app: "Gmail", action: "send email", fields: [] } } };
  const line = needsLine(n);
  assert.deepEqual(line, { title: "Send email?", sub: "Gmail · Deposit", button: "Allow" });
  assert.doesNotMatch(JSON.stringify(line), /phone|desktop|mac/i);
});

test("an agent's question is answered in its session", () => {
  const n = { kind: "agent", agent: "codex", title: "Swipe strip", machine_name: "build-box", question: { text: "Keep the old endpoint?" } };
  assert.deepEqual(needsLine(n), { title: "Keep the old endpoint?", sub: "Codex · Swipe strip", button: "Answer" });
});

test("sections pair up and empty ones drop out", () => {
  const full = { running: [1], needs_you: [1], updates: [1], coming_up: [1] };
  assert.deepEqual(homeRows(full), [["running", "needs"], ["updates", "coming"]]);
  assert.deepEqual(homeRows({ ...full, running: [], updates: null }), [["needs"], ["coming"]]);
  assert.deepEqual(homeRows({ running: null, needs_you: [], updates: [], coming_up: null }), []);
});

test("a wide Home puts what to act on left and machines right", () => {
  const full = { machines: [], running: [1], needs_you: [1], updates: [1], coming_up: [1] };
  assert.deepEqual(homeColumns(full), { left: ["needs", "running", "updates"], right: ["machines", "coming"] });
  assert.deepEqual(homeColumns({ ...full, running: [], coming_up: null }), { left: ["needs", "updates"], right: ["machines"] });
});

test("a wide Home keeps one column when a side would be empty", () => {
  assert.equal(homeColumns({ machines: [], running: [], needs_you: null, updates: [], coming_up: [1] }), null);
  assert.equal(homeColumns({ machines: null, running: null, needs_you: [1], updates: [1], coming_up: null }), null);
});
