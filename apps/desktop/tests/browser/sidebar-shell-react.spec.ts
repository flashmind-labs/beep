import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("sidebar navigation keeps focus and scroll during Home updates", async ({ page }) => {
  await openDesktop(page);
  const home = page.locator("#side").getByRole("button", { name: "Home", exact: true });
  await expect(home).toBeVisible();
  const scroll = await page.locator("#side-content").evaluate(element => { element.style.maxHeight = "80px"; element.scrollTop = 30; return element.scrollTop; });
  expect(scroll).toBeGreaterThan(0);
  await home.evaluate(element => (element as HTMLElement).focus({ preventScroll: true }));
  await home.evaluate(element => { (window as any).__sidebarHome = element; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);
  await page.evaluate(() => window.__navigation.notify("home"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length)).toBeGreaterThan(reads);
  await expect(home).toBeFocused();
  expect(await page.locator("#side-content").evaluate(element => element.scrollTop)).toBe(scroll);
  expect(await home.evaluate(element => element === (window as any).__sidebarHome)).toBe(true);
  const toggle = page.getByRole("button", { name: "Collapse sidebar" });
  await toggle.click();
  await expect(page.getByRole("button", { name: "Expand sidebar" })).toBeVisible();
  await expect(home.locator("svg")).toBeVisible();
  await page.getByRole("button", { name: "Expand sidebar" }).click();
  await expect(home.getByText("Home")).toBeVisible();
});

test("sidebar group rows keep focus on refresh", async ({ page }) => {
  await openDesktop(page);
  const settings = page.locator("#side .cc-grp.views").getByRole("button", { name: "Settings", exact: true });
  await expect(settings).toBeVisible();
  await settings.focus();
  await settings.evaluate(element => { (window as any).__settingsRow = element; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);
  await page.evaluate(() => window.__navigation.notify("home"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length)).toBeGreaterThan(reads);
  await expect(settings).toBeFocused();
  expect(await settings.evaluate(element => element === (window as any).__settingsRow)).toBe(true);
});

test("machine sidebar rows remain focused when another machine changes", async ({ page }) => {
  const machine = (id: string, name: string) => ({ id, name, online: true, can_host: true, seen_at: 0, version: "1", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents: 0, stats: null });
  const home = { machines: [machine("one", "Build box"), machine("two", "Test box")], projects: [], running: [], needs_you: [], coming_up: null, updates: null };
  await openDesktop(page, false, { center_home: null });
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, home);
  const row = page.locator("#side .cc-grp.machines").getByRole("button", { name: "Test box" });
  await expect(row).toBeVisible();
  await row.focus();
  await row.evaluate(element => { (window as any).__sidebarMachine = element; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, { ...home, machines: [{ ...home.machines[0], agents: 2 }, home.machines[1]] });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length)).toBeGreaterThan(reads);
  await expect(page.locator("#side .cc-grp.machines").getByRole("button", { name: "Build box" })).toContainText("2");
  await expect(row).toBeFocused();
  expect(await row.evaluate(element => element === (window as any).__sidebarMachine)).toBe(true);
});
