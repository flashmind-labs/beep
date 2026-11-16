import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("Home lists the sessions that ended today under Done today", async ({ page }) => {
  const now = Date.now();
  const session = (id: string, status: string, last_activity: number) => ({ machine_id: "m1", machine_name: "build-box", id, agent: "claude", title: `Session ${id}`,
    cwd: "/srv/project", status, doing: null, started_at: last_activity - 1000, last_activity, external: false, reviewer: false, parent: null });
  const home = { machines: [], projects: [], running: [], needs_you: [] };
  await openDesktop(page, false, { center_home: home, session_moves: [], session_copies: [],
    projects_sessions: [session("today", "ended", now), session("old", "ended", now - 3 * 86_400_000)] });
  await expect(page.locator(".bd-lane.hd", { hasText: "Done today" })).toHaveText("Done today1");
  await expect(page.locator(".bd-rows").getByText("Session today")).toBeVisible();
  await expect(page.locator(".bd-rows").getByText("Session old")).toHaveCount(0);
});

test("Home's board gives each project a row, and a review is opened on its host or handed to an agent", async ({ page }) => {
  const pull = { host: "example.test", repo: "other/site", number: 12, kind: "pull_request", title: "Move the blog", url: "https://example.test/other/site/pull/12",
    updated_at: 1, authored: false, assigned: false, review_requested: true, draft: false, review: "none", checks_failing: false, comments: 0 };
  await openDesktop(page, false, { git_items: { items: [pull], hosts: [] } });

  // The project that waits on the user comes first.
  await expect(page.locator(".bd-rows > .pj")).toHaveText(["other/site", projectName]);
  // Above its row, the full width of the board.
  const [name, board] = await Promise.all([page.locator(".bd-rows > .pj").first().boundingBox(), page.locator(".bd-rows").boundingBox()]);
  expect(name?.width).toBe(board?.width);
  // The heading names the project, so the card does not.
  await expect(page.locator(".bd-card", { hasText: "#12 Move the blog" }).locator(".sub")).toHaveText("your review");
  await page.locator(".bd-card", { hasText: "#12 Move the blog" }).getByRole("button", { name: "Review" }).click();
  await expect(page.getByRole("menuitem", { name: "Open", exact: true })).toBeVisible();
  await page.getByRole("menuitem", { name: "Review with agent" }).click();
  await expect(page.locator("textarea").first()).toHaveValue(/^Review PR https:\/\/example\.test\/other\/site\/pull\/12/);
});

test("a row's project name opens the project, and a full cell scrolls", async ({ page }) => {
  const pull = (number: number) => ({ host: "example.test", repo: projectName, number, kind: "pull_request", title: `Pull ${number}`, url: `https://example.test/${projectName}/pull/${number}`,
    updated_at: number, authored: false, assigned: false, review_requested: true, draft: false, review: "none", checks_failing: false, comments: 0 });
  await openDesktop(page, false, { git_items: { items: [1, 2, 3, 4].map(pull), hosts: [] } });

  const cell = page.locator(".bd-rows > .bd-lane:not(.hd)").first();
  await expect(cell.locator(".bd-card")).toHaveCount(4);
  expect(await cell.evaluate(el => el.scrollHeight > el.clientHeight)).toBe(true);
  // A wheel notch moves one card, whatever its size.
  await cell.hover();
  await page.mouse.wheel(0, 40);
  const second = await cell.evaluate(el => {
    const [first, next] = el.querySelectorAll<HTMLElement>(".bd-card");
    return next.offsetTop - first.offsetTop;
  });
  await expect.poll(() => cell.evaluate(el => el.scrollTop)).toBe(second);

  await page.locator(".bd-rows > .pj").getByRole("button", { name: projectName }).click();
  await expect(page.locator(".pj-board")).toBeVisible();
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

test("Home scrolls a card at a time, and a cell at its first card leaves the swipe to the page", async ({ page }) => {
  const projects = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"].map(name => ({ name: `example/${name}`, url: "", machine_id: "build-box", machine_name: "Build box", folder: `/srv/${name}`, cloned: true, imported_at: 1 }));
  const running = projects.flatMap(project => [1, 2, 3, 4].map(n => ({ kind: "agent", machine_id: "build-box", machine_name: "Build box", id: `${project.name}-${n}`, agent: "codex", title: `Task ${n}`, cwd: project.folder, status: "working", doing: null, started_at: 1, last_activity: n, external: false, question: null })));
  await openDesktop(page, false, { center_home: { machines: [], projects, running, needs_you: [] }, projects_sessions: [] });
  const view = page.locator("#view"), cells = page.locator(".bd-rows > .bd-lane:not(.hd)");
  await expect(cells).toHaveCount(projects.length * 4);
  const top = () => view.evaluate(el => el.scrollTop);

  // The page steps down a project row, and keeps going while the wheel does.
  await page.locator(".bd-rows > .pj").first().hover();
  await page.mouse.wheel(0, 40);
  await expect.poll(top).toBeGreaterThan(0);
  const row = await top();
  await page.mouse.wheel(0, 40);
  await expect.poll(top).toBeGreaterThan(row);

  // Over a cell already at its first card, scrolling up moves the page.
  const cell = page.locator(".bd-rows > .bd-lane:not(.hd)").filter({ has: page.locator(".bd-card") }).last();
  await cell.hover();
  const below = await top();
  expect(below).toBeGreaterThan(0);
  expect(await cell.evaluate(el => el.scrollTop)).toBe(0);
  await page.mouse.wheel(0, -40);
  await expect.poll(top).toBeLessThan(below);
  expect(await cell.evaluate(el => el.scrollTop)).toBe(0);
});

test("Appearance picks the board or the list for Home and for projects, each on its own", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.9.0", autostart: false, agent_notices: false },
    list_credentials: [], list_github_accounts: [] });
  await expect(page.locator("#sec-needs .bd-rows")).toBeVisible();

  await pickLayout(page, "Home", "List");
  expect(await page.evaluate(() => [localStorage.getItem("beep.layout.home"), localStorage.getItem("beep.layout.project")])).toEqual(["list", null]);
  await page.locator("#side").getByRole("button", { name: "Home", exact: true }).click();
  await expect(page.locator(".bd-rows")).toHaveCount(0);
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
