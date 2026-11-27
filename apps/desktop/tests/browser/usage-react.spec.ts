import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("usage updates retain the open popover and selected period", async ({ page }) => {
  const spend = { cost_usd: 2 };
  const period = { total: spend, tokens: 0, projects: [spend], harnesses: [] };
  const usage = { limits: [{ agent: "codex", account: null, plan: "pro", machine_name: "Build box", as_of: 0, windows: [{ minutes: 300, used_percent: 20, resets_at: null, scope: null }] }], today: period, week: period, month: period };
  await openDesktop(page, false, { usage_get: usage });
  const button = page.getByRole("button", { name: /Codex/ });
  await button.click();
  const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
  await expect(pop).toBeVisible();
  await pop.getByRole("button", { name: "This week" }).click();
  await pop.evaluate(element => { element.dataset.identity = "kept"; });
  const next = { ...usage, limits: [{ ...usage.limits[0], windows: [{ ...usage.limits[0].windows[0], used_percent: 35 }] }] };
  await page.evaluate(value => { window.__navigation.respond("usage_get", value); window.__navigation.notify("usage"); }, next);
  await expect(pop.getByText("35% used")).toBeVisible();
  await expect(pop).toHaveAttribute("data-identity", "kept");
  await expect(pop.getByRole("button", { name: "This week" })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect(pop).toHaveCount(0);
});

const subscribed = (() => {
  const project = () => ({ cost_usd: 0 });
  const period = {
    total: { cost_usd: 0 },
    tokens: 39_300_000,
    daily: [{ first: "2026-03-06", last: "2026-03-06", project: "acme/widget", agent: null, cost_usd: 0, tokens: 38000000 }, { first: "2026-03-06", last: "2026-03-06", project: "acme/relay", agent: null, cost_usd: 0, tokens: 1300000 }],
    projects: [project(), project()],
    harnesses: [{ agent: "claude", subscription: true, cost_usd: null, tokens: 39_300_000, models: [{ name: "claude-sonnet", tokens: 39_300_000 }] }],
  };
  const window = (minutes: number, used: number, scope: string | null = null) => ({ minutes, used_percent: used, resets_at: null, scope });
  return {
    limits: [
      { agent: "claude", account: null, plan: "max", machine_name: "build-box", as_of: Date.now(), windows: [window(300, 0), window(10_080, 76), window(10_080, 3, "Fable")], credits: [] },
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

for (const kind of ["project", "chat"] as const) {
  test(`a detached ${kind} keeps plan usage available beside its conversation @windows`, async ({ page }, info) => {
    const session = { machine_id: "build-box", machine_name: "Build box", id: "waiting", agent: "codex", title: "Choose a title", cwd: "/srv/project", status: "asking", doing: null, question: null, assistant: kind === "chat", parent: null, last_activity: 1, started_at: 1, external: false };
    const alone = kind === "project" ? { kind, project: projectName } : { kind, machine_id: session.machine_id, session: session.id };
    await page.setViewportSize({ width: 1000, height: 700 });
    await openDesktop(page, false, {
      center_window_alone: alone, project_sessions: [session], agent_sessions: [session], usage_get: subscribed,
      agent_view: { session, entries: [], plan: [], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 0 },
    }, `${kind}-sample`);

    const row = page.locator("#rail").getByRole("button", { name: /Choose a title/ });
    await expect(row.getByText("needs you")).toBeVisible();
    if (kind === "project") await row.click();
    await expect(page.locator("#agmsg")).toBeVisible();
    await expect(page.locator("#side")).toBeHidden();
    const modifier = info.project.name === "Windows" ? "Control" : "Meta";
    await page.keyboard.press(`${modifier}+b`);
    await expect(page.locator("#bell")).toBeHidden();
    expect(await page.evaluate(() => window.__navigation.calls().some(call => call.command === "notes_list" || call.command === "notes_read"))).toBe(false);

    for (const width of [1000, 640]) {
      await page.setViewportSize({ width, height: 700 });
      const pill = page.getByRole("button", { name: /^Plan usage:/ });
      await expect(pill.locator(".us-plan").first()).toBeInViewport({ ratio: 1 });
      await pill.click();
      const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
      await expect(pop.getByText("76% used")).toBeVisible();
      await pill.click();
      await expect(pop).toBeHidden();
    }
  });
}

for (const [name, size] of [["desktop", { width: 1280, height: 900 }], ["phone", { width: 390, height: 844 }]] as const) {
  test(`the ${name} pill leads with longer-window usage and labels the five-hour reading`, async ({ page }) => {
    await page.setViewportSize(size);
    await openDesktop(page, false, { usage_get: subscribed });
    const pill = page.getByRole("button", { name: /^Plan usage:/ });
    await expect(pill).toBeInViewport({ ratio: 1 });
    await expect(page.locator("#bell button")).toBeInViewport({ ratio: 1 });
    if (name === "phone") await expect(page.locator(".mb-set")).toBeInViewport({ ratio: 1 });
    await expect(pill.locator(".us-plan")).toHaveText(["76%", "Fable3%", "20%", "4%5h12%"]);
    await expect(pill.getByText("5h", { exact: true })).toBeInViewport({ ratio: 1 });
    await expect(pill.locator(".us-plan svg")).toHaveCount(3);
    await expect(pill).not.toContainText("Gemini Models");
    await expect(pill.locator(".us-plan").last()).toHaveAttribute("title", "Antigravity 4% used (This week, Gemini Models), 12% used (5 hours, Gemini Models)");
    await expect(pill.locator(".us-plan").last().locator("image")).toHaveAttribute("href", "antigravity.svg");
    await expect(pill.locator(".bar")).toHaveCount(0);
    await expect(pill.locator("b.warn")).toHaveText("76%");
    if (name === "desktop") expect(await pill.ariaSnapshot()).toMatchSnapshot("plan-usage-bar.txt");

    await pill.click();
    const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
    await expect(pop.getByText("76% used")).toBeVisible();
    await expect(pop.getByText("5 hours", { exact: true }).locator("..")).toContainText("0% used");
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
  await expect(pill.locator(".us-plan")).toHaveCount(3);
  await expect(pill.locator(".us-plan").last()).toHaveText("Work5h40%");
  await expect(pill.locator(".us-plan").last()).toHaveAttribute("title", "Claude Code Work 40% used (5 hours)");
  await pill.click();
  await expect(page.getByRole("dialog", { name: "Plan usage and cost" }).locator("h5", { hasText: "Work" })).toHaveCount(1);
});

test("a provider balance shows whole dollars in the pill and cents in the popover", async ({ page }) => {
  const period = { total: { cost_usd: 0 }, tokens: 0, projects: [], harnesses: [] };
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

test("the usage popup narrows spend to a harness or a project", async ({ page }) => {
  const cell = (project: string | null, agent: string, model: string, cost_usd: number, tokens: number, subscription = false) => ({ project, agent, model, subscription, cost_usd, tokens });
  const row = (project: string, cost_usd: number, tokens: number) => ({ first: "2026-03-06", last: "2026-03-06", project, agent: null, cost_usd, tokens });
  const cells = [
    cell("acme/widget", "claude", "claude-opus", 6, 6000), cell("acme/relay", "codex", "gpt-codex", 3, 3000),
    cell("acme/relay", "claude", "claude-opus", 1, 1000), cell("acme/widget", "claude", "claude-sonnet", 4, 40000, true),
  ];
  const period = {
    total: { cost_usd: 10 }, tokens: 50_000, projects: [], cells,
    daily: [row("acme/widget", 6, 46_000), row("acme/relay", 4, 4000)],
    harnesses: [
      { agent: "claude", subscription: false, cost_usd: 7, tokens: 7000, models: [{ name: "claude-opus", tokens: 7000 }] },
      { agent: "codex", subscription: false, cost_usd: 3, tokens: 3000, models: [{ name: "gpt-codex", tokens: 3000 }] },
      { agent: "claude", subscription: true, cost_usd: 4, tokens: 40_000, models: [{ name: "claude-sonnet", tokens: 40_000 }] },
    ],
  };
  await openDesktop(page, false, { usage_get: { limits: [], today: period, week: period, month: period } });
  await page.getByRole("button", { name: "Cost today: $10.00" }).click();
  const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
  const harness = pop.getByRole("group", { name: "Harness" }), project = pop.getByRole("group", { name: "Project" });
  await expect(pop.getByRole("table", { name: "By project" })).toHaveCount(0);
  await expect(pop.getByRole("table", { name: "Daily usage" })).toBeVisible();

  await harness.getByRole("button", { name: "Codex" }).click();
  await expect(pop.getByRole("table", { name: "Daily usage" })).toHaveCount(0);
  await expect(pop.getByRole("table", { name: "By harness" }).getByRole("row", { name: /Claude/ })).toHaveCount(0);
  await expect(pop.getByRole("table", { name: "By model" }).getByRole("row", { name: /gpt-codex/ })).toBeVisible();
  await expect(pop.getByRole("table", { name: "By project" }).getByRole("row", { name: /acme\/relay/ })).toBeVisible();
  await expect(pop.getByRole("table", { name: "By project" }).getByRole("row", { name: /acme\/widget/ })).toHaveCount(0);

  await harness.getByRole("button", { name: "All harnesses" }).click();
  await project.getByRole("button", { name: "acme/widget" }).click();
  await expect(pop.getByRole("table", { name: "Daily usage" }).getByRole("row", { name: /acme\/relay/ })).toHaveCount(0);
  await expect(pop.getByRole("table", { name: "By harness" }).getByRole("row", { name: /Claude Code, subscription/ })).toContainText("~$4");
  await expect(pop.getByRole("row", { name: /^Total / })).toContainText("$6.00");
  expect(await pop.getByRole("table", { name: "By harness" }).ariaSnapshot()).toMatchSnapshot("filtered-by-harness.txt");

  await page.evaluate(value => { window.__navigation.respond("usage_get", value); window.__navigation.notify("usage"); }, { limits: [], today: { ...period, cells: cells.filter(c => c.project !== "acme/widget") }, week: period, month: period });
  await expect(project.getByRole("button", { name: "acme/widget" })).toHaveCount(0);
  await expect(pop.getByRole("table", { name: "By project" })).toHaveCount(0);
});
