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

  await page.locator('.cc-top button[aria-label="New agent"]').tap();
  await expect(page.locator("#agdraft textarea")).toBeVisible();
});
