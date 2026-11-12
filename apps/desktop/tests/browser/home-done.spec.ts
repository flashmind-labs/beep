import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("Home lists the sessions that ended today under Done today", async ({ page }) => {
  const now = Date.now();
  const session = (id: string, status: string, last_activity: number) => ({ machine_id: "m1", machine_name: "build-box", id, agent: "claude", title: `Session ${id}`,
    cwd: "/srv/project", status, doing: null, started_at: last_activity - 1000, last_activity, external: false, reviewer: false, parent: null });
  const home = { machines: [], projects: [], running: [], needs_you: [] };
  await openDesktop(page, false, { center_home: home, session_moves: [], session_copies: [],
    projects_sessions: [session("today", "ended", now), session("old", "ended", now - 3 * 86_400_000)] });
  const done = page.locator(".bd-lane", { hasText: "Done today" });
  await expect(done.getByText("Session today")).toBeVisible();
  await expect(done.getByText("Session old")).toHaveCount(0);
});

/// Picks `layout` for `name` in Settings, under Appearance, and closes Settings.
async function pickLayout(page: Page, name: string, layout: string) {
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Appearance", exact: true }).click();
  await settings.getByRole("radiogroup", { name: `${name} layout` }).getByRole("radio", { name: layout, exact: true }).click();
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
}

test("Appearance picks the board or the list for Home and for projects, each on its own", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.9.0", autostart: false, agent_notices: false },
    list_credentials: [], list_github_accounts: [] });
  await expect(page.locator("#sec-needs .bd-lanes")).toBeVisible();

  await pickLayout(page, "Home", "List");
  expect(await page.evaluate(() => [localStorage.getItem("beep.layout.home"), localStorage.getItem("beep.layout.project")])).toEqual(["list", null]);
  await page.locator("#side").getByRole("button", { name: "Home", exact: true }).click();
  await expect(page.locator(".bd-lanes")).toHaveCount(0);
  await expect(page.locator("#sec-running")).toBeVisible();
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();
  await expect(page.locator("#side").getByRole("button", { name: "Running" })).toBeVisible();

  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".pj-board .bd-lanes")).toBeVisible();

  await pickLayout(page, "Projects", "List");
  await expect(page.locator(".pj-board")).toHaveCount(0);
  await expect(page.locator(".pj-split .pj-ov")).toBeVisible();
  await expect(page.locator(".pj-list").getByRole("button", { name: "Overview" })).toHaveAttribute("aria-current", "true");
});
