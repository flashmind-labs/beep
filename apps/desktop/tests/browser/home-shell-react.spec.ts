import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("Home refresh retains its tab, focus, content and scroll", async ({ page }) => {
  const home = { machines: [], projects: [], running: [], needs_you: [], coming_up: null,
    updates: Array.from({ length: 40 }, (_, id) => ({ id, summary: `Update ${id}`, app: "app", app_name: "App", at: 1 })) };
  await openDesktop(page, false, { center_home: home });
  const tab = page.getByRole("tab", { name: "Overview" });
  await expect(tab).toBeVisible();
  await page.evaluate(() => {
    const view = document.querySelector<HTMLElement>("#view")!;
    view.scrollTop = 500;
    (window as any).__homeScroll = view.scrollTop;
    (window as any).__homeTab = document.querySelector('[role="tab"]');
  });
  await tab.focus();
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, home);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length)).toBeGreaterThan(reads);
  await expect.poll(() => page.evaluate(() => document.querySelector("#view")!.scrollTop)).toBe(await page.evaluate(() => (window as any).__homeScroll));
  await expect(tab).toBeFocused();
  expect(await page.evaluate(() => document.querySelector('[role="tab"]') === (window as any).__homeTab)).toBe(true);
  await page.getByRole("tab", { name: "Machines" }).click();
  await expect(page.locator("#sec-machines")).toBeVisible();
});

test("Overview keeps unchanged update rows and focus when another update changes", async ({ page }) => {
  const home = { machines: [], projects: [], running: [], needs_you: [], coming_up: null,
    updates: [{ id: 1, summary: "First update", app: "app", app_name: "App", at: 1 }, { id: 2, summary: "Second update", app: "app", app_name: "App", at: 1 }] };
  await openDesktop(page, false, { center_home: home });
  const second = page.getByRole("button", { name: "Second update" });
  await expect(second).toBeVisible();
  await second.focus();
  await second.evaluate(element => { (window as any).__homeRow = element; });
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, { ...home, updates: [{ ...home.updates[0], summary: "Changed update" }, home.updates[1]] });
  await expect(page.getByRole("button", { name: "Changed update" })).toBeVisible();
  await expect(second).toBeFocused();
  expect(await second.evaluate(element => element === (window as any).__homeRow)).toBe(true);
});

test("Git items retain a focused row during a Home refresh", async ({ page }) => {
  const item = (number: number, title: string) => ({ host: "example.test", repo: "org/repo", number, title, kind: "issue", url: `https://example.test/org/repo/issues/${number}`, updated_at: 1, authored: false, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, comments: 0, labels: [] });
  const git = { hosts: [{ host: "example.test", forgejo: false, account: null, error: null }], items: [item(1, "First issue"), item(2, "Second issue")] };
  await openDesktop(page, false, { git_items: git });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  const second = page.getByRole("button", { name: /Second issue/ });
  await expect(second).toBeVisible();
  await second.focus();
  await second.evaluate(element => { (window as any).__gitRow = element; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);
  await page.evaluate(() => window.__navigation.notify("home"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length)).toBeGreaterThan(reads);
  await expect(second).toBeFocused();
  expect(await second.evaluate(element => element === (window as any).__gitRow)).toBe(true);
});

test("Git issue list loads more rows while scrolling", async ({ page }) => {
  const items = Array.from({ length: 40 }, (_, index) => ({ host: "example.test", repo: "org/repo", number: index + 1, title: `Issue ${index + 1}`, kind: "issue", url: `https://example.test/org/repo/issues/${index + 1}`, updated_at: 100 - index, authored: false, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, comments: 0, labels: [] }));
  await openDesktop(page, false, { git_items: { hosts: [{ host: "example.test", forgejo: false, account: null, error: null }], items } });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  await expect(page.getByRole("button", { name: /Issue 1(?:\s|$)/ })).toBeVisible();
  await expect.poll(async () => {
    await page.locator("#view").evaluate(element => { element.scrollTop = element.scrollHeight; });
    return page.getByRole("button", { name: /Issue 40(?:\s|$)/ }).count();
  }).toBe(1);
});

test("Git issue filters and selection keep their controls and choices", async ({ page }) => {
  const item = (number: number, label: string) => ({ host: "example.test", repo: "org/repo", number, title: `Issue ${number}`, kind: "issue", url: `https://example.test/org/repo/issues/${number}`, updated_at: 1, authored: false, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, comments: 0, labels: [label] });
  await openDesktop(page, false, { git_items: { hosts: [{ host: "example.test", forgejo: false, account: null, error: null }], items: [item(1, "bug"), item(2, "feature")] }, agent_hosts: [] });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  const issues = page.locator("#sec-git .cc-cols > div").nth(1);
  await issues.getByRole("button", { name: "bug", exact: true }).click();
  await expect(issues.getByRole("button", { name: /Issue 1/ })).toBeVisible();
  await expect(issues.getByRole("button", { name: /Issue 2/ })).toHaveCount(0);
  await issues.getByRole("button", { name: "Select", exact: true }).click();
  await issues.getByRole("checkbox", { name: "Pick this issue" }).check();
  await expect(issues.getByText("1 picked")).toBeVisible();
  await expect(issues.getByRole("button", { name: /Start 1/ })).toBeEnabled();
  await issues.getByRole("button", { name: "bug", exact: true }).click();
  await expect(issues.getByRole("checkbox", { name: "Pick this issue" })).toHaveCount(2);
  await expect(issues.getByText("1 picked")).toBeVisible();
});

test("Machines retains a focused terminal action when another machine changes", async ({ page }) => {
  const machine = (id: string, name: string) => ({ id, name, online: true, can_host: true, seen_at: 0, version: "1", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents: 0, stats: null });
  const home = { machines: [machine("one", "Build box"), machine("two", "Test box")], projects: [], running: [], needs_you: [], coming_up: null, updates: null };
  await openDesktop(page, false, { center_home: home, terminals_all: [] });
  await page.getByRole("tab", { name: "Machines" }).click();
  const terminal = page.locator("#sec-machines .act").nth(1).getByRole("button", { name: "New terminal" });
  await expect(terminal).toBeVisible();
  await terminal.focus();
  await terminal.evaluate(element => { (window as any).__machineAction = element; });
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, { ...home, machines: [{ ...home.machines[0], agents: 2 }, home.machines[1]] });
  await expect(page.locator("#m-one")).toContainText("Build box");
  await expect.poll(() => page.locator("#m-one + .num").textContent()).toBe("2");
  await expect(terminal).toBeFocused();
  expect(await terminal.evaluate(element => element === (window as any).__machineAction)).toBe(true);
});
