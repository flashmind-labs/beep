import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName, pushHome } from "./harness.ts";
import type { CenterTarget } from "../../src/stores/project/projectwindow.ts";

const pageErrors = new WeakMap<Page, string[]>();
test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  pageErrors.set(page, errors);
  page.on("pageerror", error => errors.push(error.message));
  await openDesktop(page);
});
test.afterEach(async ({ page }) => { expect(pageErrors.get(page) ?? []).toEqual([]); });

test("Escape restores the project's Running section after repeated native notifications", async ({ page }) => {
  await openProject(page);
  const running = page.locator('.pj-li[title="Running"]');
  await page.locator(".pj-strip button").filter({ hasText: "running" }).click();
  await expect(running).toHaveAttribute("aria-current", "true");
  await notification(page, "notice-one", "First notification");
  await notification(page, "notice-two", "Second notification");
  await page.keyboard.press("Escape");
  await expect(running).toBeVisible();
  await expect(running).toHaveAttribute("aria-current", "true");
  await expect(page.locator("#sec-project-agents")).toContainText("First notification");
});

test("a notification opened from a project draft returns to that project", { tag: "@windows" }, async ({ page }, info) => {
  await openProject(page);
  await page.keyboard.press(info.project.name === "Windows" ? "Control+n" : "Meta+n");
  await expect(page.locator("#agdraft textarea")).toBeFocused();
  await notification(page, "notice-one", "First notification");
  await page.keyboard.press("Escape");
  await expect(page.locator(".pj-strip")).toBeVisible();
  await expect(page.locator("#title")).toContainText("widget");
});

async function openProject(page: Page): Promise<void> {
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".pj-strip")).toBeVisible();
}

async function notification(page: Page, id: string, title: string): Promise<void> {
  const target: CenterTarget = { agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title,
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } };
  await page.evaluate(target => window.__navigation.open(target), target);
  await expect(page.locator("#agtitle")).toHaveText(title);
}

test("project redraw keeps Git panels attached and preserves history scroll", async ({ page }) => {
  await openProject(page);
  const branch = page.locator(".pj-strip button").filter({ hasText: "branch" });
  await branch.click();
  const tree = page.locator(".gh-tree");
  await expect(tree).toContainText("No commits yet.");

  await page.evaluate(() => window.__navigation.respond("project_git_history", {
    machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], next: null,
    commits: Array.from({ length: 50 }, (_, i) => ({ id: `commit-${i}`, parents: [], author: "Example", committed_at: 1, subject: `Commit ${i}` })),
  }));
  await page.evaluate(() => window.__navigation.respond("project_git_files", []));
  await page.evaluate(() => window.__navigation.respond("project_git_diff", { root: "/srv/project", base: "", merge_base: "", working_tree: false, files: [], additions: 0, deletions: 0, more: 0 }));
  await page.locator(".gh-nav-head").getByRole("button", { name: "Refresh" }).click();
  await expect(tree).toContainText("Commit 49");
  const before = await tree.evaluate(el => { el.scrollTop = el.scrollHeight; return el.scrollTop; });
  expect(before).toBeGreaterThan(0);
  await page.evaluate(() => {
    const nav = document.querySelector(".gh-nav");
    const detail = document.querySelector(".gh-detail");
    new MutationObserver(records => {
      if (records.some(record => [...record.removedNodes].some(node => node === nav || node === detail || node.contains(nav) || node.contains(detail)))) document.body.dataset.gitDetached = "true";
    }).observe(document.body, { childList: true, subtree: true });
  });
  await branch.click();
  await expect(tree).toHaveJSProperty("scrollTop", before);
  await expect(page.locator("body")).not.toHaveAttribute("data-git-detached", "true");
});

test("sidebar toggle preserves width and stays available across navigation", async ({ page }) => {
  const grip = page.getByRole("separator", { name: "Resize the sidebar" });
  await grip.focus();
  await page.keyboard.press("ArrowRight");
  const width = await page.locator("#side").evaluate(side => side.getBoundingClientRect().width);
  const toggle = page.getByRole("button", { name: "Collapse sidebar", exact: true });
  await toggle.focus();
  await page.keyboard.press("Enter");
  const expand = page.getByRole("button", { name: "Expand sidebar", exact: true });
  await expect(expand).toBeFocused();
  await expect(expand).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator(".cc-side-list")).toBeVisible();
  const home = page.locator("#side").getByRole("button", { name: "Home", exact: true });
  await expect(home).toBeVisible();
  await expect(home.locator(".tt")).toBeHidden();
  await expect(home).toHaveAttribute("title", "Home");
  await home.click();
  await expect(home).toHaveAttribute("aria-current", "true");
  await expect(page.locator("#side").getByRole("button", { name: "Settings", exact: true })).toBeVisible();
  await expect(grip).toBeHidden();
  await openProject(page);
  await expect(expand).toBeVisible();
  await expand.focus();
  await page.keyboard.press("Space");
  await expect(page.locator(".cc-side-list")).toBeVisible();
  await expect(grip).toBeVisible();
  await expect(toggle).toBeFocused();
  expect(await page.locator("#side").evaluate(side => side.getBoundingClientRect().width)).toBe(width);
});

test("project header actions retain focus during refresh", async ({ page }) => {
  await openProject(page);
  const more = page.locator("#tools").getByRole("button", { name: "More" });
  await expect(more).toBeVisible();
  await more.focus();
  await more.evaluate(element => { (window as any).__headerMore = element; });
  await pushHome(page);
  await expect(more).toBeFocused();
  expect(await more.evaluate(element => element === (window as any).__headerMore)).toBe(true);
  await page.evaluate(() => window.__navigation.respond("project_ignored_sync", { enabled: false, folders: [] }));
  await more.click();
  await page.getByRole("menuitem", { name: "Pin", exact: true }).click();
  await expect.poll(() => page.evaluate(project => window.__navigation.calls().some(call => call.command === "pin_project_to_top" && call.args.project === project), projectName)).toBe(true);
});

test("route changes and history preserve retained scroll", { tag: "@windows" }, async ({ page }, info) => {
  await page.evaluate(() => {
    const original = window.scrollTo.bind(window);
    (window as any).__routeScrolls = 0;
    Object.defineProperty(window, "scrollTo", { configurable: true, value: (...args: unknown[]) => { (window as any).__routeScrolls++; return Reflect.apply(original, window, args); } });
  });
  const home = page.locator("#view");
  const top = await home.evaluate(element => { const panel = element.querySelector<HTMLElement>("#sec-running"); if (panel) panel.style.minHeight = "2000px"; element.scrollTop = 300; return element.scrollTop; });
  expect(top).toBeGreaterThan(0);
  await openProject(page);
  await expect.poll(() => page.evaluate(() => (window as any).__TSR_ROUTER__?.state.resolvedLocation?.pathname)).toBe("/projects");
  const modifier = info.project.name === "Windows" ? "Control" : "Meta";
  await page.keyboard.press(`${modifier}+[`);
  await expect(page.getByRole("tab", { name: "Board" })).toBeVisible();
  await expect(home).toHaveJSProperty("scrollTop", top);
  await page.keyboard.press(`${modifier}+]`);
  await expect(page.locator(".pj-strip")).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__TSR_ROUTER__?.state.resolvedLocation?.pathname)).toBe("/projects");
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  expect(await page.evaluate(() => (window as any).__routeScrolls)).toBe(0);
});

test("mouse side buttons go back and forward", async ({ page }) => {
  const side = (button: number) => page.evaluate(button => {
    for (const type of ["mousedown", "mouseup"]) document.body.dispatchEvent(new MouseEvent(type, { button, bubbles: true, cancelable: true }));
  }, button);
  const path = () => page.evaluate(() => (window as any).__TSR_ROUTER__?.state.resolvedLocation?.pathname);
  await openProject(page);
  await expect.poll(path).toBe("/projects");
  await side(3);
  await expect.poll(path).toBe("/");
  await side(4);
  await expect.poll(path).toBe("/projects");
});

test("collapsed sidebar retains project colors and folder navigation", async ({ page }) => {
  const project = page.locator("#side .projects").getByRole("button", { name: new RegExp(`^${projectName}\\b`) });
  const folder = page.locator("#side .projects .cc-org").first();
  await expect(folder).toBeVisible();
  if (await folder.getAttribute("aria-expanded") === "false") await folder.click();
  await expect(project).toBeVisible();
  const dot = project.locator(".cc-pdot");
  const color = await dot.evaluate(el => getComputedStyle(el).backgroundColor);
  await page.getByRole("button", { name: "Collapse sidebar", exact: true }).click();
  await expect(folder).toBeVisible();
  await expect(dot).toBeVisible();
  await expect(project.locator(".tt")).toBeHidden();
  expect(await dot.evaluate(el => getComputedStyle(el).backgroundColor)).toBe(color);
  await folder.click();
  await expect(project).toBeHidden();
  await folder.click();
  await expect(project).toBeVisible();
  await project.click();
  await expect(page.locator("#title")).toContainText("widget");
  await expect(page.getByRole("button", { name: "Expand sidebar", exact: true })).toBeVisible();
});

test("a project draft fits a narrow chat pane without scrolling sideways", { tag: "@windows" }, async ({ page }, info) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await openProject(page);
  await page.keyboard.press(info.project.name === "Windows" ? "Control+n" : "Meta+n");
  await expect(page.locator("#agdraft .agc-intro")).toBeVisible();
  const body = page.locator("#agbody");
  expect(await body.evaluate(node => node.scrollWidth - node.clientWidth)).toBe(0);
});
