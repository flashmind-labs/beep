import test from "node:test";
import assert from "node:assert/strict";
import { agentsLine, wslHosts } from "../src/stores/machines/wsl-state.ts";
import type { MachineEnv, WslDistro } from "../src/parts/machines/machine-state.ts";

const distro = (over: Partial<WslDistro> = {}): WslDistro => ({ name: "Ubuntu", agents: [], node: null, machine_id: null, kept: false, problem: null, probed: true, ...over });
const env = (os: string, wsl: WslDistro[]): MachineEnv => ({ os, os_version: "", arch: "x86_64", hostname: "host", cpus: 1, memory_mb: 1, home: "/path/to/work", tools: [], logins: [], avds: [], gpus: [], wsl });

test("only Windows machines with distributions offer them", () => {
  const machines = [
    { id: "desk", env: env("windows", [distro()]) },
    { id: "empty", env: env("windows", []) },
    { id: "box", env: env("linux", [distro()]) },
    { id: "new", env: null },
  ];
  assert.deepEqual(wslHosts(machines).map(m => m.id), ["desk"]);
});

test("the agents read by their names", () => {
  assert.equal(agentsLine({ agents: ["claude", "codex"], probed: true }), "Claude Code, Codex");
  assert.equal(agentsLine({ agents: [], probed: true }), "No coding agents found");
  assert.equal(agentsLine({ agents: [], probed: false }), "Not looked into yet");
});
