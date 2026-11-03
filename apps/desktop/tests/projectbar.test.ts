import test from "node:test";
import assert from "node:assert/strict";
import { barCheckout, barPlace, barPrompt } from "../src/projectbar.ts";

const HOSTS = [
  { machine_id: "m", machine_name: "mac-box", agents: ["claude"] },
  { machine_id: "b", machine_name: "build-box", agents: ["codex", "claude"] },
  { machine_id: "n", machine_name: "nas-box", agents: [] },
];
const PLACES = [
  { machine: "m", cwd: "/Users/user/acme/widget" },
  { machine: "b", cwd: "/home/user/acme/widget" },
  { machine: "n", cwd: "/srv/acme/widget" },
];

test("Auto starts on the first machine by name with a checkout and an agent", () => {
  assert.deepEqual(barPlace(PLACES, HOSTS, "auto"), { machine_id: "b", machine_name: "build-box", cwd: "/home/user/acme/widget", auto: true });
  assert.deepEqual(barPlace(PLACES, HOSTS, "m"), { machine_id: "m", machine_name: "mac-box", cwd: "/Users/user/acme/widget", auto: false });
  assert.equal(barPlace(PLACES, HOSTS, "n"), null, "no coding agent there");
  assert.equal(barPlace([], HOSTS, "auto"), null, "no checkout yet");
});

test("the field asks for a task, or for a machine and folder first", () => {
  assert.equal(barPrompt("acme/widget", true), "Describe a task for a coding agent in acme/widget");
  assert.equal(barPrompt("acme/widget", false), "Pick a machine and folder for acme/widget, then describe the task");
});

test("the Folder pill says what the agent works on", () => {
  assert.deepEqual(barCheckout("new", "main"), { label: "New worktree", sub: "from main" });
  assert.deepEqual(barCheckout("branch", null), { label: "New branch", sub: "from HEAD" });
  assert.deepEqual(barCheckout("current", "main"), { label: "Current checkout", sub: "as it is" });
  assert.deepEqual(barCheckout({ worktree: "/srv/acme/widget/.worktrees/fix-login" }, "main"), { label: "fix-login", sub: "worktree" });
});
