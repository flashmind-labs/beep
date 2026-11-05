import test from "node:test";
import assert from "node:assert/strict";
import { bareVersion, newestNode, nodeBehind } from "../src/machines/machines-state.ts";

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

test("a node behind the newest is marked", () => {
  const newest = newestNode(["0.1.20", "0.1.22", "dev", "0.1.9"]);
  assert.equal(newest, "0.1.22");
  assert.equal(nodeBehind("0.1.20", newest), true);
  assert.equal(nodeBehind("0.1.22", newest), false);
  assert.equal(nodeBehind("dev", newest), false);
});

test("a tool updated this session keeps its version and loses its button", async () => {
  const { shownVersion, updateOffered, updatedNotes } = await import("../src/machines/machines-state.ts");
  const installed = new Map([["m1 flsh", "0.2.6"]]);
  const stale = [{ name: "flsh", version: "flsh 0.2.5" }];
  assert.equal(shownVersion(installed, "m1 flsh", "flsh", "flsh 0.2.5"), "0.2.6");
  assert.equal(shownVersion(installed, "m1 flsh", "flsh", "flsh 0.2.6"), "0.2.6");
  assert.equal(shownVersion(installed, "m1 flsh", "flsh", "flsh 0.2.7"), "0.2.7");
  assert.equal(shownVersion(installed, "m2 flsh", "flsh", "flsh 0.2.5"), "0.2.5");
  assert.equal(updateOffered(installed, "m1 flsh", "0.2.6"), false);
  assert.equal(updateOffered(installed, "m1 flsh", "0.2.7"), true);
  assert.equal(updateOffered(installed, "m2 flsh", "0.2.6"), true);
  assert.deepEqual(updatedNotes(installed, "m1", stale), ["Updated flsh to 0.2.6"]);
  assert.deepEqual(updatedNotes(installed, "m1", [{ name: "flsh", version: "0.2.6" }]), []);
  assert.deepEqual(updatedNotes(installed, "m2", stale), []);
});
