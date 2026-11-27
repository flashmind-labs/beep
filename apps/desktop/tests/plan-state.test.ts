import { test } from "node:test";
import assert from "node:assert/strict";
import { stepAt, type PlanStep } from "../src/stores/agents/plan-state.ts";

const step = (started_at: number | null, ended_at: number | null): PlanStep => ({ content: "", status: "", started_at, ended_at });

test("a subagent belongs to the step running when it started", () => {
  const plan = [step(10, 20), step(20, null), step(null, null)];

  assert.equal(stepAt(plan, 10), 0);
  assert.equal(stepAt(plan, 19), 0);
  assert.equal(stepAt(plan, 20), 1);
  assert.equal(stepAt(plan, 500), 1);
});

test("no step claims a subagent started before, between or after them", () => {
  const plan = [step(10, 20), step(30, 40)];

  assert.equal(stepAt(plan, 9), null);
  assert.equal(stepAt(plan, 25), null);
  assert.equal(stepAt(plan, 40), null);
  assert.equal(stepAt([step(null, null)], 5), null);
});

test("of two steps running at once, the later started takes it", () => {
  assert.equal(stepAt([step(10, null), step(15, null)], 16), 1);
  assert.equal(stepAt([step(15, null), step(10, null)], 16), 0);
});
