import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("project without a project agent retains its session and machine sections", async ({ page }) => {
  const sessions = [{ machine_id: 'build-box', machine_name: 'Build box', id: 'past-one', agent: 'codex', title: 'Past session', cwd: '/srv/project', status: 'ended', doing: null, started_at: 1, last_activity: 1, external: false, question: null }];
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, center_projects: [], project_sessions: sessions });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const layout = page.locator('.pj-cols.pj-half'), row = layout.locator('#sec-sessions .pv-row');
  await expect(row).toHaveText(/Past session/);
  await expect(layout.locator('.pm-section')).toBeVisible();
  await expect(layout.locator('#sec-project-agents')).toBeVisible();
  await expect(layout.locator('#sec-custom-agents')).toBeVisible();
  await row.focus();
  await row.evaluate(el => { (el as HTMLElement).dataset.identity = 'kept'; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'projects_on_top').length);
  await page.evaluate(() => window.__navigation.notify('projects'));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'projects_on_top').length)).toBeGreaterThan(reads);
  await expect(row).toHaveAttribute('data-identity', 'kept');
  await expect(row).toBeFocused();
  await expect(layout.locator('#sec-custom-agents')).toHaveCSS('margin-top', '14px');
  await page.evaluate(() => { window.__navigation.emit('link-state', 'unlinked'); window.__navigation.notify('projects'); });
  await expect(page.locator('.pj-split')).toBeVisible();
  await expect(page.locator('.pj-split #sec-sessions .pv-row')).toContainText('Past session');
  const agentReads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'center_projects').length);
  await page.evaluate(() => window.__navigation.emit('link-state', 'linked'));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'center_projects').length)).toBeGreaterThan(agentReads);
  await page.evaluate(() => window.__navigation.notify('projects'));
  await expect(layout.locator('#sec-sessions .pv-row')).toContainText('Past session');
  await page.evaluate(() => { window.__navigation.emit('link-state', 'unlinked'); window.__navigation.notify('projects'); });
  await expect(page.locator('.pj-split #sec-sessions .pv-row')).toContainText('Past session');
});

test("project navigation keeps scroll and focus while the selected section changes", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const nav = page.locator(".pj-split:not(.gh-layout) > .pj-list");
  const running = nav.locator('.pj-li[title="Running"]');
  await expect(running).toBeVisible();
  await nav.evaluate(el => { el.style.maxHeight = "90px"; el.scrollTop = el.scrollHeight; });
  const top = await nav.evaluate(el => el.scrollTop);
  expect(top).toBeGreaterThan(0);
  await running.focus();
  await running.evaluate(el => { el.dataset.identity = "kept"; });
  const before = await nav.evaluate(el => el.scrollTop);
  await page.locator(".pj-strip button").filter({ hasText: "Machines" }).evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(nav).toHaveJSProperty("scrollTop", before);
  await expect(running).toHaveAttribute("data-identity", "kept");
  await expect(running).toBeFocused();
  await expect(nav.locator('.pj-li[title="Machines"]')).toHaveAttribute("aria-current", "true");
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  await expect(page.locator(".gh-nav")).toBeVisible();
  await page.locator(".gh-back").click();
  await expect(nav).toBeVisible();
  await expect(running).toHaveAttribute("data-identity", "kept");
});

test("project pull request controls keep keyboard focus and row menus work", async ({ page }) => {
  const now = Date.now();
  const prs = [
    { number: 1, title: "First PR", url: "https://example.test/pull/1", author: "Example", draft: false, mine: false, created_at: now - 1000, updated_at: now, labels: ["bug"] },
    { number: 2, title: "Second PR", url: "https://example.test/pull/2", author: "Example", draft: false, mine: false, created_at: now, updated_at: now, labels: ["feature"] },
  ];
  const issues = [{ number: 3, title: "Example issue", url: "https://example.test/issues/3", author: "Example", opened_at: now, comments: 0, labels: ["bug"] }];
  await openDesktop(page, false, { center_project_lists: { issues, prs, issues_error: null, prs_error: null, synced_at: now }, center_project_threads: [], center_thread_chats: [] });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const nav = page.locator(".pj-split:not(.gh-layout) > .pj-list");
  const first = nav.locator('.pj-li[title="First PR"]');
  await expect(first).toBeVisible();
  const order = nav.getByRole("button", { name: "Newest first" });
  await order.focus();
  await page.keyboard.press("Enter");
  await expect(nav.getByRole("button", { name: "Oldest first" })).toBeFocused();
  const sorted = nav.getByRole("button", { name: "Oldest first" });
  await sorted.evaluate(el => { el.dataset.identity = "kept"; });
  const labels = nav.locator(".pj-labels").first();
  await labels.getByRole("button", { name: "bug" }).click();
  await expect(first).toBeVisible();
  await expect(sorted).toHaveAttribute("data-identity", "kept");
  await expect(nav.locator('.pj-li[title="Second PR"]')).toHaveCount(0);
  await first.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "#1 First PR" });
  await menu.getByRole("menuitem", { name: "Open on forge" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url").map(call => call.args.url))).toContain("https://example.test/pull/1");
  await nav.locator('.pj-li[title="Issues"]').click();
  await page.locator('.pj-det .pj-li[title="Example issue"]').click({ button: "right" });
  await expect(page.getByRole("menu", { name: "#3 Example issue" }).getByRole("menuitem", { name: "Work on it" })).toBeVisible();
});


test("switching projects creates navigation for the active project", async ({ page }) => {
  const first = { name: projectName, url: "https://example.test/example/widget", machine_id: "build-box", machine_name: "build-box", folder: "/srv/project", cloned: true, imported_at: 1 };
  const second = { ...first, name: "example/other", url: "https://example.test/example/other", folder: "/srv/other" };
  await openDesktop(page, false, { center_home: { machines: [], projects: [first, second], running: [], needs_you: [], coming_up: null, updates: null } });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), first.name);
  const nav = page.locator(".pj-split:not(.gh-layout) > .pj-list");
  const firstRow = nav.locator('.pj-li[title="Machines"]');
  await expect(firstRow).toBeVisible();
  await firstRow.evaluate(el => { el.dataset.projectIdentity = "first"; });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), second.name);
  await expect(nav.locator('.pj-li[title="Machines"]')).toBeVisible();
  await expect(nav.locator('.pj-li[title="Machines"]')).not.toHaveAttribute("data-project-identity", "first");
  await nav.locator('.pj-li[title="Machines"]').click();
  await expect(nav.locator('.pj-li[title="Machines"]')).toHaveAttribute("aria-current", "true");
});
