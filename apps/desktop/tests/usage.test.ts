import test from "node:test";
import assert from "node:assert/strict";
import { harnessLine, harnessName, projectExtra, planUsed, windowUsed } from "../src/center/usage.ts";

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
  assert.equal(harnessLine(covered), "1023.5M");
  assert.equal(harnessLine({ ...covered, cost_usd: 361.58 }), "1023.5M");
  const project = { project: "widget", cost_usd: 2.1, unpriced: [], metered_tokens: 800_000, subscription: [{ name: "claude", tokens: 210_000_000 }] };
  assert.equal(projectExtra(project, true), "0.8M");
  assert.equal(projectExtra({ ...project, unpriced: [{ name: "codex", tokens: 1_100_000 }] }, false), "1.1M Codex tokens");
});
