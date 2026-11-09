import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";
import type { CenterTarget } from "../../src/project/projectwindow.ts";

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
  await running.click();
  await expect(running).toHaveAttribute("aria-current", "true");
  await notification(page, "notice-one", "First notification");
  await notification(page, "notice-two", "Second notification");
  await page.keyboard.press("Escape");
  await expect(running).toBeVisible();
  await expect(running).toHaveAttribute("aria-current", "true");
  await expect(page.locator("#sec-project-agents")).toContainText("First notification");
});

test("a notification opened from a project draft returns to that project", async ({ page }, info) => {
  await openProject(page);
  await page.keyboard.press(info.project.name === "Windows" ? "Control+n" : "Meta+n");
  await expect(page.locator("#agdraft textarea")).toBeFocused();
  await notification(page, "notice-one", "First notification");
  await page.keyboard.press("Escape");
  await expect(page.locator('.pj-li[title="Running"]')).toBeVisible();
  await expect(page.locator("#title")).toContainText("widget");
});

async function openProject(page: Page): Promise<void> {
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator('.pj-li[title="Running"]')).toBeVisible();
}

async function notification(page: Page, id: string, title: string): Promise<void> {
  const target: CenterTarget = { agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title,
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } };
  await page.evaluate(target => window.__navigation.open(target), target);
  await expect(page.locator("#agtitle")).toHaveText(title);
}

test("Git panels stay visible across unchanged project redraws", async ({ page }) => {
  await openProject(page);
  const branch = page.locator(".pj-strip button").filter({ hasText: "branch" });
  await expect(branch).toBeVisible();
  await branch.click();
  await expect(page.locator(".gh-nav")).toBeVisible();
  await expect(page.locator(".gh-tree")).toContainText("No commits yet.");

  for (let i = 0; i < 5; ++i) {
    await branch.click();
    await expect(page.locator(".gh-nav")).toBeVisible();
    await expect(page.locator(".gh-detail")).toBeVisible();
    await expect(page.locator(".gh-tree")).toContainText("No commits yet.");
  }
});

test("project redraw keeps Git panels attached and preserves history scroll", async ({ page }) => {
  await page.evaluate(() => window.__navigation.respond("project_git_history", {
    machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], next: null,
    commits: Array.from({ length: 50 }, (_, i) => ({ id: `commit-${i}`, parents: [], author: "Example", committed_at: 1, subject: `Commit ${i}` })),
  }));
  await page.evaluate(() => window.__navigation.respond("project_git_files", []));
  await page.evaluate(() => window.__navigation.respond("project_git_diff", { root: "/srv/project", base: "", merge_base: "", working_tree: false, files: [], additions: 0, deletions: 0, more: 0 }));
  await openProject(page);
  const branch = page.locator(".pj-strip button").filter({ hasText: "branch" });
  await branch.click();
  const tree = page.locator(".gh-tree");
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
