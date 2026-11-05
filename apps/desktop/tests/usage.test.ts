import test from "node:test";
import assert from "node:assert/strict";
import { harnessLine, harnessName, projectExtra, planUsed, todayLine, windowUsed } from "../src/center/usage.ts";
import { tokenCount } from "../src/ui/format.ts";

test("a plan has used what its tightest window has used", () => {
  const plan = { windows: [{ used_percent: 38 }, { used_percent: 76.4 }] };
  assert.equal(planUsed(plan), 76);
  assert.equal(planUsed({ windows: [] }), null);
  assert.equal(windowUsed({ used_percent: 130 }), 100);
  assert.equal(windowUsed({ used_percent: -5 }), 0);
});

test("subscription use shows as tokens under its own line", () => {
  const covered = { agent: "claude", subscription: true, cost_usd: null, tokens: 1_023_500_000, models: [] };
  assert.equal(harnessName(covered), "Claude Code, subscription");
  assert.equal(harnessLine(covered), "1B");
  assert.equal(harnessLine({ ...covered, cost_usd: 361.58 }), "1B");
  const project = { project: "widget", cost_usd: 2.1, unpriced: [], metered_tokens: 800_000, subscription: [{ name: "claude", tokens: 210_000_000 }] };
  assert.equal(projectExtra(project, true), "210.8M");
  assert.equal(projectExtra({ ...project, unpriced: [{ name: "codex", tokens: 1_100_000 }] }, false), "1.1M Codex tokens");
});

test("token counts move up a unit before they read 1000", () => {
  assert.equal(tokenCount(999_000_000), "999M");
  assert.equal(tokenCount(999_960_000), "1B");
  assert.equal(tokenCount(1_055_000_000), "1.1B");
  assert.equal(tokenCount(999_960_000_000), "1T");
  assert.equal(tokenCount(2_400_000_000_000), "2.4T");
});

test("the pill shows dollars paid, else the tokens plans covered", () => {
  assert.equal(todayLine({ total: { cost_usd: 0 }, tokens: 1_055_000_000 }), "1.1B");
  assert.match(todayLine({ total: { cost_usd: 0.42 }, tokens: 1_055_000_000 }), /0\.42/);
  assert.match(todayLine({ total: { cost_usd: 0 }, tokens: 0 }), /0/);
});
