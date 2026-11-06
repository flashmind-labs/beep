import test from "node:test";
import assert from "node:assert/strict";
import { LAST_MACHINE, removeBlock, workingIn } from "../src/project/removemachine-state.ts";
import type { RunningAgent } from "../src/agents/running-state.ts";

const here = { kind: "here" as const }, none = { kind: "none" as const };
const box = { name: "build-box" };

test("the last machine with the project keeps it", () => {
  assert.equal(removeBlock("acme/widget", [here, none, none], box, 0), LAST_MACHINE);
  assert.equal(removeBlock("acme/widget", [here, here, none], box, 0), null);
});

test("an agent working there is stopped first", () => {
  assert.equal(removeBlock("acme/widget", [here, here], box, 1), "An agent is working in acme/widget on build-box. Stop it first.");
});

const agent = (machine_id: string, cwd: string): RunningAgent => ({ kind: "agent", machine_id, machine_name: machine_id, id: cwd, agent: "claude", title: null, cwd, status: "working", doing: null, started_at: 0, last_activity: 0, external: false });

test("agents count in the folder and under it, on that machine only", () => {
  const running = [agent("a", "/srv/widget"), agent("a", "/srv/widget/.worktrees/fix"), agent("a", "/srv/widget-old"), agent("b", "/srv/widget")];
  assert.equal(workingIn(running, "a", "/srv/widget"), 2);
  assert.equal(workingIn(running, "c", "/srv/widget"), 0);
});
