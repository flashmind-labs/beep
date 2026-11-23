import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("usage updates retain the open popover and selected period", async ({ page }) => {
  const spend = { project: "Example", cost_usd: 2, unpriced: [] };
  const period = { total: spend, tokens: 0, projects: [spend], harnesses: [] };
  const usage = { limits: [{ agent: "codex", account: null, plan: "pro", machine_name: "Build box", as_of: 0, windows: [{ minutes: 300, used_percent: 20, resets_at: null, scope: null }] }], today: period, week: period, month: period };
  await openDesktop(page, false, { usage_get: usage });
  const button = page.getByRole("button", { name: /Codex/ });
  await button.click();
  const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
  await expect(pop).toBeVisible();
  await pop.getByRole("button", { name: "This week" }).click();
  await pop.evaluate(element => { element.dataset.identity = "kept"; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "usage_get").length);
  await page.evaluate(value => { window.__navigation.respond("usage_get", value); window.__navigation.notify("usage"); }, usage);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "usage_get").length)).toBeGreaterThan(reads);
  await expect(pop).toHaveAttribute("data-identity", "kept");
  await expect(pop.getByRole("button", { name: "This week" })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect(pop).toHaveCount(0);
});

const subscribed = (() => {
  const project = (name: string, tokens: number) => ({ project: name, cost_usd: 0, unpriced: [], metered_tokens: 0, subscription: [{ name: "claude", tokens }] });
  const period = {
    total: { project: "", cost_usd: 0, unpriced: [], metered_tokens: 0, subscription: [] },
    tokens: 39_300_000,
    daily: [{ first: "2026-03-06", last: "2026-03-06", project: "acme/widget", agent: null, cost_usd: 0, tokens: 38000000 }, { first: "2026-03-06", last: "2026-03-06", project: "acme/relay", agent: null, cost_usd: 0, tokens: 1300000 }],
    projects: [project("acme/widget", 38_000_000), project("acme/relay", 1_300_000)],
    harnesses: [{ agent: "claude", subscription: true, cost_usd: null, tokens: 39_300_000, models: [{ name: "claude-sonnet", tokens: 39_300_000 }] }],
  };
  const window = (minutes: number, used: number, scope: string | null = null) => ({ minutes, used_percent: used, resets_at: null, scope });
  return {
    limits: [
      { agent: "claude", account: null, plan: "max", machine_name: "build-box", as_of: Date.now(), windows: [window(300, 38), window(10_080, 76)], credits: [] },
      { agent: "codex", account: null, plan: "pro", machine_name: "build-box", as_of: Date.now(), windows: [window(10_080, 20)], credits: [] },
      { agent: "agy", account: null, plan: null, machine_name: "build-box", as_of: Date.now(), windows: [
        window(300, 12, "Gemini Models"), window(300, 0, "Claude and GPT models"),
        window(10_080, 4, "Gemini Models"), window(10_080, 0, "Claude and GPT models"),
      ], credits: [] },
    ],
    historical: [{ first: "2026-03-03", last: "2026-03-04", project: "acme/legacy", agent: "flsh", cost_usd: 3.25, tokens: 7000 }],
    today: period, week: period, month: period,
  };
})();

for (const [name, size] of [["desktop", { width: 1280, height: 900 }], ["phone", { width: 390, height: 844 }]] as const) {
  test(`the ${name} pill shows each harness's binding share and the popover its values`, async ({ page }) => {
    await page.setViewportSize(size);
    await openDesktop(page, false, { usage_get: subscribed });
    const pill = page.getByRole("button", { name: "Plan usage: Claude Code 76%, Codex 20%, Antigravity 12%" });
    await expect(pill).toBeVisible();
    await expect(pill.locator(".us-plan")).toHaveCount(3);
    await expect(pill.locator(".us-plan svg")).toHaveCount(3);
    await expect(pill).not.toContainText("Gemini Models");
    await expect(pill.locator(".us-plan").nth(2)).toHaveAttribute("title", "Antigravity: 12% of 5 hours, Gemini Models used");
    await expect(pill.locator(".us-plan").nth(2).locator("image")).toHaveAttribute("href", "antigravity.svg");
    await expect(pill.locator(".bar")).toHaveCount(0);
    await expect(pill.locator("b.warn")).toHaveText("76%");
    await expect(pill.locator(".us-plan").nth(1).locator("b")).not.toHaveClass(/warn/);

    await pill.click();
    const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
    await expect(pop.getByText("76% used")).toBeVisible();
    await expect(pop.getByText("5 hours, Gemini Models", { exact: true })).toBeVisible();
    await expect(pop.getByText("5 hours, Claude and GPT models", { exact: true })).toBeVisible();
    await expect(pop.getByText("This week, Gemini Models", { exact: true })).toBeVisible();
    await expect(pop.getByRole("row").filter({ hasText: "acme/widget" }).getByText("38M")).toBeVisible();
    await expect(pop.getByRole("row").filter({ hasText: "Claude Code, subscription" }).getByText("39.3M")).toBeVisible();
    await expect(pop.getByRole("row").filter({ hasText: "claude-sonnet" }).getByText("39.3M")).toBeVisible();
    await expect(pop.getByRole("row").filter({ hasText: "Total" }).getByText("39.3M")).toBeVisible();
    const history = pop.getByRole("table", { name: "Historical spend (daily split unknown)" });
    await expect(history.getByRole("cell", { name: "$3.25" })).toBeVisible();
    await expect(history.getByRole("columnheader")).toHaveText(["Session dates", "Project", "Harness", "Spend", "Tokens"]);
    const daily = pop.getByRole("table", { name: "Daily usage" });
    await expect(daily.getByRole("columnheader")).toHaveText(["Date", "Project", "Spend", "Tokens"]);
    await expect(daily.getByText("acme/legacy")).toHaveCount(0);
    expect(await daily.ariaSnapshot()).toMatchSnapshot("daily-spend-table.txt");
    expect(await history.ariaSnapshot()).toMatchSnapshot("historical-spend-table.txt");
    expect(await pop.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
    await expect(pop.locator(".us-note")).toHaveCount(0);
    await page.screenshot({ path: `../../.artifacts/usage-${name}.png` });
  });
}

test("each named account's plan shows apart, under its name", async ({ page }) => {
  const window = { minutes: 300, used_percent: 40, resets_at: null, scope: null };
  const work = { ...subscribed.limits[0], account: "Work", windows: [window] };
  await openDesktop(page, false, { usage_get: { ...subscribed, limits: [subscribed.limits[0], work] } });
  const pill = page.getByRole("button", { name: /^Plan usage:/ });
  await expect(pill.locator(".us-plan")).toHaveCount(2);
  await expect(pill.locator(".us-plan").nth(1)).toHaveAttribute("title", "Claude Code Work: 40% of 5 hours used");
  await pill.click();
  await expect(page.getByRole("dialog", { name: "Plan usage and cost" }).locator("h5", { hasText: "Work" })).toHaveCount(1);
});

test("a provider balance shows whole dollars in the pill and cents in the popover", async ({ page }) => {
  const period = { total: { project: "", cost_usd: 0, unpriced: [] }, tokens: 0, projects: [], harnesses: [] };
  const credit = { provider: "openrouter", scope: "account", unit: "usd", remaining: "622.3218671", total: "1000", used: "377.6781329", resets: null };
  const written = { ...credit, remaining_short: "$622", remaining_full: "$622.32", used_full: "$377.68", total_full: "$1,000.00" };
  const usage = (credits: object[]) => ({ limits: [{ agent: "flsh", account: null, plan: null, machine_name: "build-box", as_of: Date.now(), windows: [], credits }], today: period, week: period, month: period });
  await openDesktop(page, false, { usage_get: usage([written]) });
  const pill = page.locator(".us-pill");
  await expect(pill.locator(".us-x svg")).toHaveCount(1);
  await expect(pill).not.toContainText("OpenRouter");
  await expect(pill).toContainText("$622 left");
  await expect(pill.locator(".us-x").first()).toHaveAttribute("title", "OpenRouter: $622.32 remaining");
  await pill.click();
  const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
  await expect(pop).toContainText("$622.32 remaining");
  await expect(pop).toContainText("$377.68 used · $1,000.00 total");

  // A desktop without the written amounts shows the provider's own.
  await page.evaluate(value => { window.__navigation.respond("usage_get", value); window.__navigation.notify("usage"); }, usage([{ ...credit, remaining_short: null, remaining_full: null, used_full: null, total_full: null }]));
  await expect(pop).toContainText("$622.3218671 remaining");
  await expect(pill).toContainText("$622.3218671 left");
});

test("the weekly table shows each day's spend and keeps the historical balance separate", async ({ page }) => {
  const row = (date: string, cost: number, tokens: number) => ({
    first: date, last: date, project: "sample/editor", agent: null, cost_usd: cost, tokens,
  });
  const today = { ...subscribed.today, daily: [row("2026-03-06", 0.85, 3200)],
    total: { ...subscribed.today.total, cost_usd: 0.85 }, tokens: 3200, harnesses: [], projects: [] };
  const week = { ...today, daily: [...today.daily, row("2026-03-05", 2.15, 7000)],
    total: { ...today.total, cost_usd: 3 }, tokens: 10200,
    harnesses: [{ agent: "flsh", subscription: false, cost_usd: 3, tokens: 10200, models: [] }] };
  await openDesktop(page, false, { usage_get: { limits: [], today, week, month: week, historical: subscribed.historical } });
  await page.getByRole("button", { name: "Cost today: $0.85" }).click();
  const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
  const daily = pop.getByRole("table", { name: "Daily usage" });
  await expect(daily.getByRole("cell", { name: "$2.15" })).toHaveCount(0);
  await pop.getByRole("button", { name: "This week" }).click();
  await expect(daily.getByRole("cell", { name: "$2.15" })).toBeVisible();
  await expect(pop.getByRole("row", { name: /^Total / }).getByRole("cell", { name: "$3.00" })).toBeVisible();
  await expect(pop.getByRole("table", { name: /Historical spend/ }).getByRole("cell", { name: "$3.25" })).toBeVisible();
  await pop.screenshot({ path: "../../.artifacts/daily-usage-table.png" });
});
