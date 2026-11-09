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
