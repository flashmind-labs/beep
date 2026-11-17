import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("closing a session opened from Home opens its next running session", async ({ page }, info) => {
  await openDesktop(page);
  await page.locator("#sec-running .bd-card").first().click();
  await expect(page.locator("#agfield")).toBeVisible();

  await page.keyboard.press(info.project.name === "Windows" ? "Control+Shift+W" : "Meta+Shift+W");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "agent_steer" && call.args.steer === "close"))).toBe(true);
  await expect(page.locator(".ag-head .tt")).toContainText("Second notification");
  await expect(page.getByRole("tab", { name: "Board" })).toHaveCount(0);
});

test("Esc from a session opened from Home still goes back to Home", async ({ page }) => {
  await openDesktop(page);
  await page.locator("#sec-running .bd-card").first().click();
  await expect(page.locator("#agfield")).toBeVisible();
  await page.locator("#agfield textarea").blur();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("tab", { name: "Board" })).toBeVisible();
});
