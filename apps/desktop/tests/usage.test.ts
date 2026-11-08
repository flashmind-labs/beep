import test from "node:test";
import assert from "node:assert/strict";
import { creditAmount, creditScope, hasUsage, harnessLine, harnessName, modelTotals, projectExtra, planUsed, todayLine, windowUsed } from "../src/center/usage.ts";
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
  assert.equal(harnessLine({ ...covered, cost_usd: 361.58 }), "~$362 · 1B");
  assert.equal(harnessLine({ ...covered, cost_usd: 0 }), "1B");
  assert.equal(harnessLine({ ...covered, subscription: false, cost_usd: 5.85, tokens: 600_000 }), "$5.85 · 0.6M");
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

test("model totals combine harnesses and billing types, sorted by tokens", () => {
  const base = { agent: "claude", cost_usd: null, tokens: 0 };
  assert.deepEqual(modelTotals([
    { ...base, models: [{ name: "model-a", tokens: 10 }, { name: "model-b", tokens: 20 }] },
    { ...base, subscription: true, models: [{ name: "model-a", tokens: 15 }, { name: "unknown", tokens: 0 }] },
  ]), [{ name: "model-a", tokens: 25 }, { name: "model-b", tokens: 20 }]);
  assert.deepEqual(modelTotals([]), []);
});

test("credits preserve zero, precision and unknown balances without counting as spend", () => {
  const credit = { provider: "openrouter", scope: "api_key", unit: "usd", remaining: "0", total: null, used: "42.123456", resets: null };
  assert.equal(creditAmount(credit, credit.remaining), "$0");
  assert.equal(creditAmount(credit, credit.used), "$42.123456");
  assert.equal(creditAmount(credit, credit.total), "unavailable");
  assert.equal(creditScope("api_key"), "API key limit");
  const empty = { total: { project: "", cost_usd: 0, unpriced: [] }, tokens: 0, projects: [], harnesses: [] };
  assert.ok(hasUsage({ limits: [{ agent: "flsh", plan: null, machine_name: "build-box", as_of: 0, windows: [], credits: [credit] }], today: empty, week: empty, month: null }));
});
