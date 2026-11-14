import test from "node:test";
import assert from "node:assert/strict";
import { agentsLine, wslHosts, wslState } from "../src/machines/wsl-state.ts";
import type { MachineEnv, WslDistro } from "../src/machines/machine-state.ts";

const distro = (over: Partial<WslDistro> = {}): WslDistro => ({ name: "Ubuntu", agents: [], node: null, machine_id: null, kept: false, problem: null, ...over });
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
  assert.deepEqual(wslState(distro({ machine_id: "wsl-node", kept: true }), machines), { step: "joined", name: "Ubuntu on desk", kept: true, problem: null });
});

test("the agents read by their names", () => {
  assert.equal(agentsLine(["claude", "codex"]), "Claude Code, Codex");
  assert.equal(agentsLine([]), "No coding agents found");
});
