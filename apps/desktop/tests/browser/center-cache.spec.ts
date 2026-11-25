import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const machine = { machine_id: "build-box", machine_name: "Build box", folder: "/srv/project", status: "in_sync", files: 18, bytes: 1024, pending: 0, conflicts: [], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, can_start_over: false, git_only: false };
const terminal = { id: "shell-one", project: "", machine_id: "build-box", machine_name: "build-box", name: "shell-one", command: "shell", cwd: "/srv/project", state: "running", code: null, started_at: 1 };
const responses = {
  project_sync: [{ project: projectName, machine_id: "build-box", status: "in_sync", files: 18, pending: 0, error: null }],
  project_sync_detail: { project: projectName, pinned: false, paused: false, machines: [machine] },
  terminals_all: [terminal],
};

/// Commands that only follow topics or projects, log or keep the cache: everything else reads.
const QUIET = new Set(["mesh_watch", "mesh_topics", "follow_project", "unfollow_project", "log_webview", "desktop_timings", "desktop_ui_metrics", "center_keep_pages", "desktop_trace_enabled", "center_pending"]);
const reads = (page: Page, from: number) => page.evaluate(([from, quiet]) => window.__navigation.calls().slice(from).map(call => call.command).filter(command => !quiet.includes(command)), [from, [...QUIET]] as const);
const mark = (page: Page) => page.evaluate(() => window.__navigation.calls().length);

async function round(page: Page) {
  const side = page.locator("#side");
  await side.getByRole("button", { name: /^Terminals\b/ }).click();
  await expect(page.locator(".tl-list").getByRole("button", { name: /shell-one/ })).toBeVisible();
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".cc-view:visible .pj-board .bd-lanes")).toBeVisible();
  await side.getByRole("button", { name: "Home", exact: true }).click();
  await expect(page.getByRole("tab", { name: "Board" })).toBeVisible();
}

test("pages opened again draw what the window holds without reading", async ({ page }) => {
  await openDesktop(page, false, responses);
  await expect(page.getByRole("tab", { name: "Board" })).toBeVisible();
  await round(page);
  // Let the first reads and the idle prefetch settle.
  await page.waitForTimeout(5000);

  const before = await mark(page);
  await round(page);
  await round(page);
  await page.waitForTimeout(1500);
  // The project's session list is let go when its page goes away and read again when it returns.
  expect(await reads(page, before)).toEqual(["project_sessions", "project_sessions_forget", "project_sessions", "project_sessions_forget"]);

  // A terminal change comes with its push, and nothing is read.
  const changed = await mark(page);
  await page.evaluate(() => window.__navigation.notify("terminals"));
  await page.waitForTimeout(1500);
  expect(await reads(page, changed)).toEqual([]);

  // Opening the project reads its sessions; its terminals came with the push.
  const opened = await mark(page);
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".cc-view:visible .pj-board .bd-lanes")).toBeVisible();
  await page.waitForTimeout(1500);
  expect((await reads(page, opened)).toSorted()).toEqual(["project_sessions"]);
});
