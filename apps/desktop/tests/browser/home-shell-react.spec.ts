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
