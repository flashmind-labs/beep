import test from "node:test";
import assert from "node:assert/strict";
import { hasUsage, harnessCost, harnessName, modelTotals, pillPlans, resetName, showsDollars, todayLine, windowUsed } from "../src/center/usage-state.ts";
import { creditAmount, creditScope } from "../src/ui/credits.ts";
import { tokenCount } from "../src/ui/format.ts";

test("a window's used share is clamped to 0 to 100", () => {
  assert.equal(windowUsed({ used_percent: 130 }), 100);
  assert.equal(windowUsed({ used_percent: -5 }), 0);
});

test("subscription use shows as tokens under its own line", () => {
  const covered = { agent: "claude", subscription: true, cost_usd: null, tokens: 1_023_500_000, models: [] };
  assert.equal(harnessName(covered), "Claude Code, subscription");
  assert.equal(harnessCost(covered), "");
  assert.equal(harnessCost({ ...covered, cost_usd: 361.58 }), "~$362");
  assert.equal(harnessCost({ ...covered, cost_usd: 0 }), "");
  assert.equal(harnessCost({ ...covered, subscription: false, cost_usd: 5.85, tokens: 600_000 }), "$5.85");

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
  const credit = { provider: "openrouter", scope: "api_key", unit: "usd", remaining: "0", total: null, used: "42.123456", resets: null, remaining_short: null, remaining_full: null, used_full: null, total_full: null };
  assert.equal(creditAmount(credit, credit.remaining), "$0");
  assert.equal(creditAmount(credit, credit.used), "$42.123456");
  assert.equal(creditAmount(credit, credit.total), "unavailable");
  assert.equal(creditScope("api_key"), "API key limit");
  const empty = { total: { cost_usd: 0 }, tokens: 0, projects: [], daily: [], harnesses: [] };
  assert.ok(hasUsage({ historical: [], limits: [{ agent: "flsh", account: null, plan: null, machine_name: "build-box", as_of: 0, windows: [], credits: [credit] }], today: empty, week: empty, month: empty }));
});

const usageWindow = (minutes: number, used_percent: number, scope: string | null = null) => ({ minutes, used_percent, resets_at: null, scope });
const plan = (agent: string, windows: ReturnType<typeof usageWindow>[]) => ({ agent, account: null, plan: null, machine_name: "build-box", as_of: 0, windows });

test("the pill shows the highest longer-window reading before the five-hour reading", () => {
  const short = usageWindow(300, 83);
  const week = usageWindow(10_080, 26);
  const codex = usageWindow(300, 1);
  assert.deepEqual(pillPlans([
    plan("claude", [short, week, usageWindow(43_200, 12)]),
    plan("codex", [codex]),
    plan("agy", [usageWindow(300, 0.9)]),
    plan("flsh", []),
  ]), [
    { agent: "claude", account: null, model: null, windows: [week, short] },
    { agent: "codex", account: null, model: null, windows: [codex] },
  ]);
});

test("the pill groups readings for a model used past 1% beside its plan", () => {
  const short = usageWindow(300, 0);
  const week = usageWindow(10_080, 20);
  const fableWeek = usageWindow(10_080, 91, "Fable");
  const fableShort = usageWindow(300, 0.9, "Fable");
  assert.deepEqual(pillPlans([plan("claude", [short, week, fableWeek, fableShort, usageWindow(10_080, 1, "Opus")])]), [
    { agent: "claude", account: null, model: null, windows: [week] },
    { agent: "claude", account: null, model: "Fable", windows: [fableWeek] },
  ]);
});

test("a plan with only grouped windows shows the highest usage for each period under the harness mark", () => {
  const week = usageWindow(10_080, 12, "Claude and GPT models");
  const short = usageWindow(300, 25, "Gemini Models");
  assert.deepEqual(pillPlans([plan("agy", [usageWindow(10_080, 4, "Gemini Models"), week, short, usageWindow(300, 0, "Claude and GPT models")])]), [
    { agent: "agy", account: null, model: null, windows: [week, short] },
  ]);
});

test("the note on dollars shows only when a row shows dollars", () => {
  const total = { cost_usd: 0 };
  const covered = { agent: "claude", subscription: true, cost_usd: null, tokens: 5_000, models: [] };
  assert.equal(showsDollars({ total, tokens: 5_000, projects: [], daily: [], harnesses: [covered] }), false);
  assert.equal(showsDollars({ total, tokens: 5_000, projects: [], daily: [], harnesses: [{ ...covered, cost_usd: 61 }] }), true);
  assert.equal(showsDollars({ total: { ...total, cost_usd: 1.5 }, tokens: 0, projects: [], daily: [], harnesses: [] }), true);
});

test("a reset reads as a countdown down to the minute", () => {
  const min = 60_000;
  assert.equal(resetName(0, 0), "in 1m");
  assert.equal(resetName(37 * min - 1, 0), "in 37m");
  assert.equal(resetName(4 * 60 * min, 0), "in 4h");
  assert.equal(resetName((4 * 60 + 12) * min, 0), "in 4h 12m");
  assert.equal(resetName((56 * 60 + 30) * min, 0), "in 2d 8h");
  assert.equal(resetName(2 * 24 * 60 * min, 0), "in 2d");
});
