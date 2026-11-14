import test from "node:test";
import assert from "node:assert/strict";
import { WslWorkStore, agentsLine, wslHosts, wslState } from "../src/machines/wsl-state.ts";
import type { MachineEnv, WslDistro } from "../src/machines/machine-state.ts";

const distro = (over: Partial<WslDistro> = {}): WslDistro => ({ name: "Ubuntu", agents: [], node: null, machine_id: null, kept: false, problem: null, probed: true, ...over });
const env = (os: string, wsl: WslDistro[]): MachineEnv => ({ os, os_version: "", arch: "x86_64", hostname: "host", cpus: 1, memory_mb: 1, home: "/path/to/work", tools: [], logins: [], avds: [], wsl });

test("only Windows machines with distributions offer them", () => {
  const machines = [
    { id: "desk", env: env("windows", [distro()]) },
    { id: "empty", env: env("windows", []) },
    { id: "box", env: env("linux", [distro()]) },
    { id: "new", env: null },
  ];
  assert.deepEqual(wslHosts(machines, null).map(m => m.id), ["desk"]);
  assert.deepEqual(wslHosts(machines, "box"), []);
});

test("a distribution is added, starting, or runs as its machine", () => {
  const machines = [{ id: "wsl-node", name: "Ubuntu on desk" }];
  assert.deepEqual(wslState(distro(), machines), { step: "add" });
  assert.deepEqual(wslState(distro({ machine_id: "gone" }), machines), { step: "add" });
  assert.deepEqual(wslState(distro({ kept: true, problem: "exited" }), machines), { step: "starting", problem: "exited" });
  assert.deepEqual(wslState(distro({ machine_id: "wsl-node", kept: true }), machines), { step: "running", name: "Ubuntu on desk", problem: null });
  assert.deepEqual(wslState(distro({ machine_id: "wsl-node" }), machines), { step: "off", name: "Ubuntu on desk" });
});

test("the agents read by their names", () => {
  assert.equal(agentsLine({ agents: ["claude", "codex"], probed: true }), "Claude Code, Codex");
  assert.equal(agentsLine({ agents: [], probed: true }), "No coding agents found");
  assert.equal(agentsLine({ agents: [], probed: false }), "Not looked into yet");
});

test("the work store tells its watchers and keeps the last snapshot until it changes", () => {
  const store = new WslWorkStore();
  let told = 0;
  const stop = store.subscribe(() => { told++; });
  const before = store.snapshot();
  store.set("desk Ubuntu", { step: "busy", text: "Adding Ubuntu" });
  assert.notEqual(store.snapshot(), before);
  assert.deepEqual(store.snapshot().get("desk Ubuntu"), { step: "busy", text: "Adding Ubuntu" });
  store.set("desk Ubuntu", null);
  stop();
  store.set("desk Debian", { step: "added" });
  assert.equal(told, 2);
});
