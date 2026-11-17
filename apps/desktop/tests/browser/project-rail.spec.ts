import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("a project's session swaps the sidebar for the project's sessions", async ({ page }) => {
  const sessions = Array.from({ length: 4 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100 - i, external: false, question: null, assistant: false }));
  await openDesktop(page, false, { project_sessions: sessions });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  const win = page.locator(".cwin"), rail = page.locator("#rail"), toggle = page.locator(".cc-side-toggle");
  await expect(rail).toBeHidden();

  await page.locator(".pj-det .pv-row").filter({ hasText: "Past session 1" }).click();
  await expect(rail).toBeVisible();
  await expect(win).toHaveClass(/side-collapsed/);
  await expect(rail.locator(".pv-row")).toHaveCount(4);
  await expect(rail.locator('.pv-row[aria-current="true"]')).toHaveText(/Past session 1/);

  await rail.getByPlaceholder("Filter sessions").fill("session 3");
  await expect(rail.locator(".pv-row")).toHaveText([/Past session 3/]);

  await toggle.click();
  await expect(rail).toBeHidden();
  await expect(win).not.toHaveClass(/side-collapsed/);
  await toggle.click();
  await expect(rail).toBeVisible();

  await rail.locator(".rail-back").click();
  await expect(rail).toBeHidden();
  await expect(win).not.toHaveClass(/side-collapsed/);
});
