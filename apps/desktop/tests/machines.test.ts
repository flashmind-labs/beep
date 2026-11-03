import test from "node:test";
import assert from "node:assert/strict";
import { bareVersion, coderColumns, coders, newestNode, nodeBehind, specLine, toolChips } from "../src/machines-state.ts";

test("versions are bare, without the tool's name or a leading v", () => {
  assert.equal(bareVersion("claude", "2.1.312 (Claude Code)"), "2.1.312");
  assert.equal(bareVersion("codex", "codex-cli 0.162.1"), "0.162.1");
  assert.equal(bareVersion("codex", "codex codex-cli 0.162.1"), "0.162.1");
  assert.equal(bareVersion("opencode", "opencode v2.2.5"), "2.2.5");
  assert.equal(bareVersion("opencode", "opencode opencode v2.2.5"), "2.2.5");
  assert.equal(bareVersion("opencode", "1.3.13"), "1.3.13");
  assert.equal(bareVersion("go", "go version go1.22.0 linux/amd64"), "1.22.0");
  assert.equal(bareVersion("docker", "Docker version 27.3.1, build ce12230"), "27.3.1");
  assert.equal(bareVersion("tool", "no version here"), "no version here");
  assert.equal(bareVersion("tool", null), null);
});

const named = (...names: string[]) => names.map(name => ({ name, version: null }));

test("useful tools come first and the rest collapse into +N", () => {
  const studio = named("git", "gh", "teacli", "docker", "cargo", "node", "python3", "go", "just", "flsh", "kubectl", "adb", "xcodebuild", "brew");
  assert.deepEqual(toolChips(studio), {
    shown: ["gh", "teacli", "docker", "xcodebuild", "adb", "kubectl"],
    more: ["brew", "git", "cargo", "node", "python3", "go", "just"],
  });
  const box = named("git", "gh", "teacli", "docker", "cargo", "node", "python3", "go", "just", "pi", "flsh", "adb");
  assert.deepEqual(toolChips(box), { shown: ["gh", "teacli", "docker", "adb", "git", "cargo"], more: ["node", "python3", "go", "just"] });
  // Seven fit without a "+N".
  assert.deepEqual(toolChips(named("git", "gh", "node", "just", "go", "cargo", "brew")), {
    shown: ["gh", "brew", "git", "node", "just", "go", "cargo"],
    more: [],
  });
});

test("coding agents keep one column order across machines", () => {
  assert.deepEqual(coders(named("git", "opencode", "codex", "claude")).map(t => t.name), ["claude", "codex", "opencode"]);
  assert.deepEqual(coderColumns([named("claude", "codex"), named("opencode", "claude", "git")]), ["claude", "codex", "opencode"]);
});

test("the spec line is short", () => {
  assert.equal(specLine({ os_version: "macOS 26.3", arch: "aarch64", cpus: 14, memory_mb: 36_864 }), "macOS 26.3 · arm64 · 14 CPU · 36 GB");
});

test("a node behind the newest is marked", () => {
  const newest = newestNode(["0.1.20", "0.1.22", "dev", "0.1.9"]);
  assert.equal(newest, "0.1.22");
  assert.equal(nodeBehind("0.1.20", newest), true);
  assert.equal(nodeBehind("0.1.22", newest), false);
  assert.equal(nodeBehind("dev", newest), false);
});
