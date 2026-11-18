import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("shortcuts filter keeps focus and closes with Escape", { tag: "@windows" }, async ({ page }, testInfo) => {
  await openDesktop(page, true);
  const modifier = testInfo.project.name === "Windows" ? "Control" : "Meta";
  await page.keyboard.press(`${modifier}+/`);
  const filter = page.getByRole("textbox", { name: "Filter shortcuts" });
  await expect(filter).toBeFocused();
  await filter.fill("no matching shortcut exists");
  await expect(page.getByText("No shortcut matches.")).toBeVisible();
  await expect(filter).toBeFocused();
  await filter.fill("Search");
  await expect(page.locator(".ks-row").filter({ hasText: "Search" }).first()).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(filter).toHaveCount(0);
});
