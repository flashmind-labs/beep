import { expect, test } from "@playwright/test";
import { openDesktop, pushHome } from "./harness.ts";

test("Home refresh retains its tab, focus, content and scroll", async ({ page }) => {
  const home = { machines: [], projects: [], running: [], needs_you: [], coming_up: null,
    updates: Array.from({ length: 40 }, (_, id) => ({ id, summary: `Update ${id}`, app: "app", app_name: "App", at: 1 })) };
  await openDesktop(page, false, { center_home: home });
  const tab = page.getByRole("tab", { name: "Board" });
  await expect(tab).toBeVisible();
  await page.evaluate(() => {
    const view = document.querySelector<HTMLElement>("#view")!;
    view.scrollTop = 500;
    (window as any).__homeScroll = view.scrollTop;
    (window as any).__homeTab = document.querySelector('[role="tab"]');
  });
  await tab.focus();
  await pushHome(page, home);
  await expect.poll(() => page.evaluate(() => document.querySelector("#view")!.scrollTop)).toBe(await page.evaluate(() => (window as any).__homeScroll));
  await expect(tab).toBeFocused();
  expect(await page.evaluate(() => document.querySelector('[role="tab"]') === (window as any).__homeTab)).toBe(true);
  await page.getByRole("tab", { name: "Machines" }).click();
  await expect(page.locator("#sec-machines")).toBeVisible();
});

test("Git items retain a focused row during a Home refresh", async ({ page }) => {
  const item = (number: number, title: string) => ({ host: "example.test", repo: "org/repo", number, title, kind: "issue", url: `https://example.test/org/repo/issues/${number}`, updated_at: 1, authored: false, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, labels: [] });
  const git = { hosts: [{ error: null }], items: [item(1, "First issue"), item(2, "Second issue")] };
  await openDesktop(page, false, { git_items: git });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  const second = page.getByRole("button", { name: /Second issue/ });
  await expect(second).toBeVisible();
  await second.focus();
  await second.evaluate(element => { (window as any).__gitRow = element; });
  await pushHome(page);
  await expect(second).toBeFocused();
  expect(await second.evaluate(element => element === (window as any).__gitRow)).toBe(true);
});

test("Git issue list loads more rows while scrolling", async ({ page }) => {
  const items = Array.from({ length: 40 }, (_, index) => ({ host: "example.test", repo: "org/repo", number: index + 1, title: `Issue ${index + 1}`, kind: "issue", url: `https://example.test/org/repo/issues/${index + 1}`, updated_at: 100 - index, authored: false, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, labels: [] }));
  await openDesktop(page, false, { git_items: { hosts: [{ error: null }], items } });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  await expect(page.getByRole("button", { name: /Issue 1(?:\s|$)/ })).toBeVisible();
  await expect.poll(async () => {
    await page.locator("#view").evaluate(element => { element.scrollTop = element.scrollHeight; });
    return page.getByRole("button", { name: /Issue 40(?:\s|$)/ }).count();
  }).toBe(1);
});

test("Home lists pull requests and issues as a card per repository, with their buttons under the pointer", async ({ page }) => {
  const item = (number: number, kind: string, repo = "example/widget") => ({ host: "example.test", repo, number, title: `Change ${number}`, kind, url: `https://example.test/${repo}/${number}`, updated_at: number, authored: false, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, labels: ["bug"] });
  const many = Array.from({ length: 12 }, (_, index) => item(10 + index, "issue", "other/long"));
  await openDesktop(page, false, { git_items: { hosts: [{ error: null }], items: [item(1, "pull_request"), item(2, "issue"), item(3, "issue"), ...many] }, agent_hosts: [] });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  const cards = page.locator("#sec-git .bd-projects > .pl"), card = cards.filter({ hasText: "example/widget" });
  await expect(cards).toHaveCount(2);
  await expect(card.locator(".pl-row.repo")).toContainText("1 PR · 2 issues");
  await expect(card.locator(".it")).toHaveCount(3);

  const row = card.locator(".it", { hasText: "Change 1" }), review = row.getByRole("button", { name: "Review", exact: true });
  await expect(review).toHaveCount(0);
  await row.hover();
  await expect(review).toBeVisible();

  // A card with many items keeps its height and scrolls them.
  const long = cards.filter({ hasText: "other/long" }).locator(".pl-body");
  await expect(long.locator(".it")).toHaveCount(12);
  expect(await long.evaluate(body => body.scrollHeight > body.clientHeight)).toBe(true);
});

test("Home PRs and issues defers desktop PR session lookups until hover and handles many authored PRs on phone without freezing", async ({ page }) => {
  const prs = Array.from({ length: 24 }, (_, index) => ({
    host: "example.test", repo: "example/widget", number: index + 1, title: `Authored PR ${index + 1}`,
    kind: "pull_request", url: `https://example.test/example/widget/pull/${index + 1}`, updated_at: 1000 - index,
    authored: true, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, labels: [],
  }));
  await openDesktop(page, false, { git_items: { hosts: [{ error: null }], items: prs }, agent_hosts: [], center_find_prompts: [] });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  await expect(page.locator("#sec-git .it")).toHaveCount(24);

  // Desktop does not start PR session lookups for unhovered rows.
  const lookups = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_find_prompts").map(call => call.args.query));
  expect(await lookups()).toEqual([]);

  const first = page.locator("#sec-git .it", { hasText: "Authored PR 1" }).first();
  await first.hover();
  await expect(first.getByRole("button", { name: "Work on PR", exact: true })).toBeVisible();
  expect(await lookups()).toEqual(["https://example.test/example/widget/pull/1"]);

  // Phone layout mounts row actions up front and resolves all 24 authored PRs without an exponential redraw storm.
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page, false, { git_items: { hosts: [{ error: null }], items: prs }, agent_hosts: [], center_find_prompts: [] });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  await expect(page.locator("#sec-git .it")).toHaveCount(24);
  await expect(page.locator("#sec-git .it").first().getByRole("button", { name: "Work on PR", exact: true })).toBeVisible();
  expect((await lookups()).length).toBe(24);
});

test("Home keeps pull requests and issues to this week or one day, a day back at a time", async ({ page }) => {
  // A Thursday at noon: Monday of its week is three days back.
  const now = new Date(2026, 0, 15, 12).getTime(), hour = 3_600_000;
  await page.clock.setFixedTime(now);
  const item = (number: number, ago: number) => ({ host: "example.test", repo: "example/widget", number, title: `Change ${number}`, kind: "issue", url: `https://example.test/example/widget/${number}`, updated_at: now - ago, authored: true, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, labels: [] });
  await openDesktop(page, false, { git_items: { hosts: [], items: [item(1, hour), item(2, 26 * hour), item(3, 72 * hour), item(4, 240 * hour)] }, agent_hosts: [] });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  const rows = page.locator("#sec-git .it"), period = (name: string | RegExp) => page.getByRole("radio", { name });
  await expect(rows).toHaveCount(4);

  await period("This week").click();
  await expect(rows).toHaveCount(3);

  await period("Today").click();
  await expect(rows).toHaveCount(1);
  await expect(rows).toContainText("Change 1");
  await expect(page.getByRole("button", { name: "Next day" })).toBeDisabled();

  await page.getByRole("button", { name: "Previous day" }).click();
  await expect(period("Yesterday")).toHaveAttribute("aria-checked", "true");
  await expect(rows).toContainText("Change 2");

  await page.getByRole("button", { name: "Previous day" }).click();
  await expect(period(/Jan 13/)).toHaveAttribute("aria-checked", "true");
  await expect(rows).toHaveCount(0);
  await expect(page.locator("#sec-git")).toContainText("No pull request or issue changed in that time.");

  await page.getByRole("button", { name: "Next day" }).click();
  await expect(rows).toContainText("Change 2");
  await period("All").click();
  await expect(rows).toHaveCount(4);
});

test("Machines retains a focused terminal action when another machine changes", async ({ page }) => {
  const machine = (id: string, name: string) => ({ id, name, online: true, can_host: true, seen_at: 0, version: "1", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: null });
  const home = { machines: [machine("one", "Build box"), machine("two", "Test box")], projects: [], running: [], needs_you: [] };
  await openDesktop(page, false, { center_home: home, terminals_all: [] });
  await page.getByRole("tab", { name: "Machines" }).click();
  const terminal = page.locator("#sec-machines .act").nth(1).getByRole("button", { name: "New terminal" });
  await expect(terminal).toBeVisible();
  await terminal.focus();
  await terminal.evaluate(element => { (window as any).__machineAction = element; });
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, { ...home, machines: [{ ...home.machines[0], agents: 2 }, home.machines[1]] });
  await expect(page.locator("#m-one")).toContainText("Build box");
  await expect(page.locator("#m-one .mnote")).toHaveText("2 agents");
  await expect(terminal).toBeFocused();
  expect(await terminal.evaluate(element => element === (window as any).__machineAction)).toBe(true);
});

test("Home shows unavailable and recently offline notices", async ({ page }) => {
  const home = { machines: null, projects: [], running: null, needs_you: [] };
  await openDesktop(page, false, { center_home: home });
  await expect(page.getByText("Machines and running sessions could not be read.")).toBeVisible();
  const machine = { id: "one", name: "Build box", online: false, can_host: true, seen_at: Date.now(), version: "1", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, { ...home, machines: [machine], running: [] });
  await expect(page.getByText("Build box is offline.")).toBeVisible();
});

test("A machine card folds its quiet projects and keeps the ones that need a look", async ({ page }) => {
  const stats = { sampled_at: 0, disks: [{ path: "/srv", free_bytes: 50, total_bytes: 1000 }], memory_used_mb: 1024, memory_total_mb: 4096, load: null };
  const machine = (id: string, name: string, online: boolean) => ({ id, name, online, can_host: true, seen_at: 0, version: "1", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: online ? stats : null });
  const home = { machines: [machine("one", "Build box", true), machine("two", "Test box", false)], projects: [], running: [], needs_you: [] };
  const row = (project: string, status: string) => ({ project, machine_id: "one", status, files: 10, pending: 0, error: null });
  await openDesktop(page, false, { center_home: home, terminals_all: [], project_sync: [row("example/one", "in_sync"), row("example/two", "in_sync"), row("example/three", "failed")] });
  await page.getByRole("tab", { name: "Machines" }).click();
  const card = page.locator("#m-one");
  await expect(card.locator(".mbar.low")).toHaveCount(1);
  await expect(card.getByRole("button", { name: /example\/three/ })).toBeVisible();
  await expect(card.getByRole("button", { name: /example\/one/ })).toHaveCount(0);
  const fold = card.getByRole("button", { name: /3 projects here/ });
  await expect(fold).toHaveAttribute("aria-expanded", "false");
  await fold.click();
  await expect(fold).toHaveAttribute("aria-expanded", "true");
  await expect(card.locator(".mgrid .cc-row")).toHaveCount(3);
  await expect(page.locator("#m-two .mstats")).toHaveCount(0);
  await expect(page.locator("#m-two .mnote")).toContainText("offline");
  await page.getByRole("tab", { name: "Board" }).click();
  await page.getByRole("tab", { name: "Machines" }).click();
  await expect(page.locator("#m-one .mfold")).toHaveAttribute("aria-expanded", "true");
});
