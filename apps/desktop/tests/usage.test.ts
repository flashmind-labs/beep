import test from "node:test";
import assert from "node:assert/strict";
import { asOf, harnessLine, hasUsage, money, planLeft, planName, resetName, shortName, todayTotal, tokenCount, unpricedLine, windowLeft, windowName, type UsageView } from "../src/usage.ts";

const empty: UsageView = { limits: [], today: [], harnesses: [], week: { project: "", cost_usd: 0, unpriced: [] } };

test("a plan has left what its tightest window has left", () => {
  const plan = { windows: [{ used_percent: 38 }, { used_percent: 76.4 }] };
  assert.equal(planLeft(plan), 24);
  assert.equal(planLeft({ windows: [] }), null);
  assert.equal(windowLeft({ used_percent: 130 }), 0);
  assert.equal(windowLeft({ used_percent: -5 }), 100);
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

test("today's total adds every project", () => {
  const total = todayTotal([
    { project: "widget", cost_usd: 3.1, unpriced: [{ name: "codex", tokens: 1_100_000 }] },
    { project: "relay", cost_usd: 1.1, unpriced: [{ name: "codex", tokens: 200_000 }] },
  ]);
  assert.equal(money(total.cost), "$4.20");
  assert.equal(total.tokens, 1_300_000);
});

test("a harness shows dollars only when it reports them", () => {
  assert.equal(harnessLine({ agent: "claude", cost_usd: 1.75, tokens: 5_000, models: [] }), "$1.75 · 5K tokens");
  assert.equal(harnessLine({ agent: "codex", cost_usd: null, tokens: 900_000, models: [] }), "0.9M tokens");
});

test("the pill hides with nothing to show or an old server", () => {
  assert.equal(hasUsage(null), false);
  assert.equal(hasUsage(empty), false);
  assert.equal(hasUsage({ ...empty, limits: [{ agent: "claude", plan: null, machine_name: "build-box", as_of: 0, windows: [] }] }), false);
  assert.equal(hasUsage({ ...empty, today: [{ project: "widget", cost_usd: 0.5, unpriced: [] }] }), true);
});
