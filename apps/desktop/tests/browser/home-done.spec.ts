import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("Home lists the sessions that ended today under Done today", async ({ page }) => {
  const now = Date.now();
  const session = (id: string, status: string, last_activity: number) => ({ machine_id: "m1", machine_name: "build-box", id, agent: "claude", title: `Session ${id}`,
    cwd: "/srv/project", status, doing: null, started_at: last_activity - 1000, last_activity, external: false, reviewer: false, parent: null });
  const home = { machines: [], projects: [], running: [], needs_you: [] };
  await openDesktop(page, false, { center_home: home, session_moves: [], session_copies: [],
    projects_sessions: [session("today", "ended", now), session("old", "ended", now - 3 * 86_400_000)] });
  await expect(page.locator("#sec-running .bd-lane.log > h4")).toHaveText("Done today1");
  await expect(page.locator("#sec-running").getByText("Session today")).toBeVisible();
  await expect(page.locator("#sec-running").getByText("Session old")).toHaveCount(0);
});

test("Home's queue opens a review on its host or hands it to an agent", async ({ page }) => {
  const pull = { host: "example.test", repo: "other/site", number: 12, kind: "pull_request", title: "Move the blog", url: "https://example.test/other/site/pull/12",
    updated_at: 1, authored: false, assigned: false, review_requested: true, draft: false, review: "none", checks_failing: false, comments: 0 };
  await openDesktop(page, false, { git_items: { items: [pull, { ...pull, repo: projectName, number: 13, title: "Fix the feed", url: `https://example.test/${projectName}/pull/13` }], hosts: [] },
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_models: { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }] } });

  const queue = page.locator("#sec-needs");
  await expect(queue.locator(".cc-row", { hasText: "#12 Move the blog" })).toContainText("other/site · your review");
  await expect(queue.locator(".cc-row", { hasText: "#12 Move the blog" }).getByRole("button", { name: "Open" })).toBeVisible();
  await queue.locator(".cc-row", { hasText: "#13 Fix the feed" }).getByRole("button", { name: "Review", exact: true }).click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await expect(picker).toBeVisible();
  // Placed whole inside the window, by its button.
  await expect.poll(() => picker.evaluate(element => {
    const box = element.getBoundingClientRect();
    const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    return box.left >= 0 && box.top >= 0 && box.right <= innerWidth && box.bottom <= innerHeight && element.style.top !== "" && !!hit && element.contains(hit);
  })).toBe(true);
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

test("Appearance picks the board or the list for projects, and Home has no layout to pick", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.9.0", autostart: false, agent_notices: false },
    list_credentials: [], list_github_accounts: [] });
  await expect(page.getByRole("tab", { name: "Board" })).toBeVisible();

  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Appearance", exact: true }).click();
  await expect(settings.getByRole("radiogroup", { name: "Projects layout" })).toBeVisible();
  await expect(settings.getByRole("radiogroup", { name: "Home layout" })).toHaveCount(0);
  await page.keyboard.press("Escape");

  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".pj-board .bd-lanes")).toBeVisible();

  await pickLayout(page, "Projects", "List");
  expect(await page.evaluate(() => localStorage.getItem("beep.layout.project"))).toBe("list");
  await expect(page.locator(".pj-board")).toHaveCount(0);
  await expect(page.locator(".pj-split .pj-ov")).toBeVisible();
  await expect(page.locator(".pj-list").getByRole("button", { name: "Overview" })).toHaveAttribute("aria-current", "true");
});
