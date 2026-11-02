import test from "node:test";
import assert from "node:assert/strict";
import type { MacControl } from "../src/home.ts";
import { agentOf, bytes, homeColumns, homeRows, machineCaps, machineDisk, machineMemory, machineNow, machineOs, needsLine, runningLine, thisMacRow } from "../src/home.ts";

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
  const r = { kind: "agent" as const, agent: "claude", title: "Fix the build", cwd: "/srv/project/widget", machine_name: "build-box" };
  assert.deepEqual(runningLine(r), { title: "Fix the build", sub: "widget · build-box" });
  assert.deepEqual(runningLine({ ...r, title: null, cwd: "" }), { title: "Claude Code", sub: "Claude Code · build-box" });
});

test("a chat approval allows from the row and names no device", () => {
  const n = { kind: "chat" as const, session_title: "Deposit", pending: { approval: { app: "Gmail", action: "send email", fields: [] } } };
  const line = needsLine(n);
  assert.deepEqual(line, { title: "Send email?", sub: "Gmail · Deposit", button: "Allow" });
  assert.doesNotMatch(JSON.stringify(line), /phone|desktop|mac/i);
});

test("an agent's question is answered in its session", () => {
  const n = { kind: "agent" as const, agent: "codex", title: "Swipe strip", machine_name: "build-box", question: { text: "Keep the old endpoint?" } };
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

test("sizes read as disks are sold", () => {
  assert.equal(bytes(850e6), "850 MB");
  assert.equal(bytes(4.3e9), "4.3 GB");
  assert.equal(bytes(212.4e9), "212 GB");
  assert.equal(bytes(1.2e12), "1.2 TB");
  assert.equal(bytes(0), "0 B");
});

test("Disk shows free space and warns under a tenth free", () => {
  const disks = [{ path: "/srv/project", free_bytes: 212e9, total_bytes: 500e9 }, { path: "/home/user", free_bytes: 4e9, total_bytes: 100e9 }];
  const d = machineDisk(machine({ stats: { disks } }));
  assert.deepEqual([d.text, d.low], ["212 GB free", false]);
  assert.equal(d.title, "/srv/project: 212 GB free of 500 GB\n/home/user: 4.0 GB free of 100 GB");
  assert.equal(machineDisk(machine({ stats: { disks: disks.slice(1) } })).low, true);
  assert.equal(machineDisk(machine()).text, "–");
  assert.equal(machineDisk(machine({ online: false, stats: { disks } })).text, "–");
});

test("Memory shows used of total in GB with the load in its title", () => {
  const stats = { memory_used_mb: 9318, memory_total_mb: 32768, load: 1.5 };
  assert.deepEqual(machineMemory(machine({ stats, env: { cpus: 8 } })), { text: "9.1 / 32 GB", title: "Load 1.50 on 8 cores" });
  assert.deepEqual(machineMemory(machine({ stats: { ...stats, load: null } })), { text: "9.1 / 32 GB", title: "" });
  assert.equal(machineMemory(machine({ stats: { ...stats, memory_total_mb: 0 } })).text, "–");
  assert.equal(machineMemory(machine()).text, "–");
});

test("This Mac gets a row until the server lists it", () => {
  const mac = (over: Partial<MacControl> = {}): Omit<MacControl, "permissions"> => ({ id: "mac-key", name: "Studio", enabled: true, status: "connecting", error: null, ...over });
  assert.equal(thisMacRow(mac(), [{ id: "mac-key" }]), null);
  assert.equal(thisMacRow(mac(), null), null);
  assert.equal(thisMacRow(null, []), null);
  assert.equal(thisMacRow(mac({ status: "unlinked" }), []), null);
  const off = thisMacRow(mac({ enabled: false, status: "off" }), [{ id: "other" }]);
  assert.equal(off!.state, "Off");
  assert.equal(off!.action, "Add");
  assert.equal(off!.name, "This Mac");
  assert.equal(off!.sub, "Studio");
  const failed = thisMacRow(mac({ status: "failed", error: "connect timed out" }), []);
  assert.equal(failed!.state, "connect timed out");
  assert.match(failed!.title, /connect timed out/);
  assert.equal(thisMacRow(mac({ status: "connected" }), [])!.state, "Connecting");
});

test("each coding agent has its badge, and an unknown one its initials", () => {
  assert.deepEqual(agentOf("opencode"), { cls: "opencode", ini: "OC", name: "OpenCode" });
  assert.equal(agentOf("claude").ini, "CC");
  assert.deepEqual(agentOf("amp"), { cls: "", ini: "AM", name: "amp" });
});
