import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("Home lists the sessions that ended today under Done today", async ({ page }) => {
  const now = Date.now();
  const session = (id: string, status: string, last_activity: number) => ({ machine_id: "m1", machine_name: "build-box", id, agent: "claude", title: `Session ${id}`,
    cwd: "/srv/project", status, doing: null, started_at: last_activity - 1000, last_activity, external: false, reviewer: false, parent: null });
  const home = { machines: [], projects: [], running: [], needs_you: [], coming_up: null, updates: [] };
  await openDesktop(page, false, { center_home: home, session_moves: [], session_copies: [],
    projects_sessions: [session("today", "ended", now), session("old", "ended", now - 3 * 86_400_000)] });
  const done = page.locator(".bd-lane", { hasText: "Done today" });
  await expect(done.getByText("Session today")).toBeVisible();
  await expect(done.getByText("Session old")).toHaveCount(0);
});

test("the layout switch turns Home's board into the list and back, and is kept", async ({ page }) => {
  const now = Date.now();
  const running = [{ kind: "agent", machine_id: "m1", machine_name: "build-box", id: "s1", agent: "claude", title: "Fix the build", cwd: "/srv/project",
    status: "working", doing: null, started_at: now - 60_000, last_activity: now, external: false }];
  const home = { machines: [], projects: [], running, needs_you: [], coming_up: null, updates: [] };
  await openDesktop(page, false, { center_home: home, projects_sessions: [] });
  await expect(page.locator(".bd-lane", { hasText: "Working" }).getByText("Fix the build")).toBeVisible();

  await page.getByRole("button", { name: "List", exact: true }).click();
  await expect(page.locator(".bd-lanes")).toHaveCount(0);
  await expect(page.locator("#sec-running").getByText("Fix the build")).toBeVisible();
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem("beep.frontLayout"))).toBe("list");

  await page.getByRole("button", { name: "Board", exact: true }).click();
  await expect(page.locator(".bd-lane", { hasText: "Working" }).getByText("Fix the build")).toBeVisible();
});

test("the layout switch turns a project's board into its Overview beside the list", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".pj-board .bd-lanes")).toBeVisible();

  await page.locator(".pj-strip").getByRole("button", { name: "List", exact: true }).click();
  await expect(page.locator(".pj-board")).toHaveCount(0);
  await expect(page.locator(".pj-split .pj-ov")).toBeVisible();
  await expect(page.locator(".pj-list").getByRole("button", { name: "Overview" })).toHaveAttribute("aria-current", "true");

  await page.locator(".pj-strip").getByRole("button", { name: "Board", exact: true }).click();
  await expect(page.locator(".pj-board .bd-lanes")).toBeVisible();
});
