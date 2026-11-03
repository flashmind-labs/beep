import test from "node:test";
import assert from "node:assert/strict";
import { asOf, harnessLine, harnessName, hasUsage, projectExtra, money, periods, planName, planUsed, resetName, shortName, tokenCount, unpricedLine, windowName, windowUsed, type Period, type UsageView } from "../src/usage.ts";

const none: Period = { total: { project: "", cost_usd: 0, unpriced: [] }, tokens: 0, projects: [], harnesses: [] };
const empty: UsageView = { limits: [], today: none, week: none, month: none };

test("a plan has used what its tightest window has used", () => {
  const plan = { windows: [{ used_percent: 38 }, { used_percent: 76.4 }] };
  assert.equal(planUsed(plan), 76);
  assert.equal(planUsed({ windows: [] }), null);
  assert.equal(windowUsed({ used_percent: 130 }), 100);
  assert.equal(windowUsed({ used_percent: -5 }), 0);
});

test("windows and plans read as people say them", () => {
  assert.equal(windowName({ minutes: 300, scope: null }), "5 hours");
  assert.equal(windowName({ minutes: 10080, scope: null }), "This week");
  assert.equal(windowName({ minutes: 10080, scope: "opus" }), "This week, Opus");
  assert.equal(windowName({ minutes: 2880, scope: null }), "2 days");
  assert.equal(planName("claude", "max"), "Max plan");
  assert.equal(planName("codex", "pro"), "ChatGPT Pro");
  assert.equal(planName("codex", null), "");
  assert.equal(shortName("claude"), "Claude");
  assert.equal(shortName("codex"), "Codex");
});

test("a reset today shows the time, a later one the weekday", () => {
  const now = new Date(2026, 10, 3, 9, 0).getTime();
  assert.equal(resetName(new Date(2026, 10, 3, 14, 30).getTime(), now), "14:30");
  assert.equal(resetName(new Date(2026, 10, 9, 8, 0).getTime(), now), "Mon");
});

test("how old a reading is", () => {
  const now = 10_000_000;
  assert.equal(asOf(now - 20_000, now), "as of just now");
  assert.equal(asOf(now - 4 * 60_000, now), "as of 4 min ago");
  assert.equal(asOf(now - 125 * 60_000, now), "as of 2 h ago");
});

test("dollars and tokens", () => {
  assert.equal(money(4.2), "$4.20");
  assert.equal(tokenCount(1_100_000), "1.1M");
  assert.equal(tokenCount(200_000), "0.2M");
  assert.equal(tokenCount(2_000_000), "2M");
  assert.equal(tokenCount(12_400), "12K");
  assert.equal(tokenCount(800), "800");
  assert.equal(unpricedLine([{ name: "codex", tokens: 1_100_000 }, { name: "gemini", tokens: 0 }]), "1.1M Codex tokens");
});

test("the month shows only when the server keeps it", () => {
  assert.deepEqual(periods(empty).map(p => p.label), ["Today", "This week", "This month"]);
  assert.deepEqual(periods({ ...empty, month: null }).map(p => p.key), ["today", "week"]);
});

test("a harness shows dollars only when it reports them", () => {
  assert.equal(harnessLine({ agent: "claude", cost_usd: 1.75, tokens: 5_000, models: [] }), "$1.75 · 5K");
  assert.equal(harnessLine({ agent: "codex", cost_usd: null, tokens: 900_000, models: [] }), "0.9M");
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

test("the pill hides with nothing to show or an old server", () => {
  assert.equal(hasUsage(null), false);
  assert.equal(hasUsage(empty), false);
  assert.equal(hasUsage({ ...empty, limits: [{ agent: "claude", plan: null, machine_name: "build-box", as_of: 0, windows: [] }] }), false);
  assert.equal(hasUsage({ ...empty, today: { ...none, projects: [{ project: "widget", cost_usd: 0.5, unpriced: [] }] } }), true);
  assert.equal(hasUsage({ ...empty, month: { ...none, tokens: 900_000 } }), true);
});
