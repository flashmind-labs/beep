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
