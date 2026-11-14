import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

test("a phone starts an agent from the + in the project's top bar", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator("body")).toHaveClass(/mobile/);
  await expect(page.locator(".cc-top #title")).toBeVisible();
  // The page keeps its room: the task bar is not on it.
  await expect(page.locator("textarea")).toHaveCount(0);

  // The board runs down to the tab bar.
  const ends = await page.evaluate(() => [document.querySelector(".pj-board")!.getBoundingClientRect().bottom, document.querySelector("#mb-tabs")!.getBoundingClientRect().top]);
  expect(ends[0]).toBe(ends[1]);

  // The forge links wait in the menu.
  await expect(page.locator(".cc-top .tools button")).toHaveCount(2);
  await page.locator('.cc-top button[aria-label="More"]').tap();
  await expect(page.getByText("Open on Forgejo")).toBeVisible();
  await page.keyboard.press("Escape");

  await page.locator('.cc-top button[aria-label="New agent"]').tap();
  await expect(page.locator("#agdraft textarea")).toBeVisible();
});
