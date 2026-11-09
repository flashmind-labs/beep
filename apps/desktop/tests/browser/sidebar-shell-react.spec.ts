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
