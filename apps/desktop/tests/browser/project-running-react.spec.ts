import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("project running agents retain their rows and open by keyboard", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "running" }).click();
  const nav = page.locator('.pj-list .pj-li[title="Running"]');
  const row = page.locator('#sec-project-agents .cc-line').filter({ hasText: 'First notification' });
  await expect(row).toBeVisible();
  const button = row.locator('button.t');
  await button.focus();
  await button.evaluate(el => { el.dataset.identity = 'kept'; });
  await nav.evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(button).toHaveAttribute('data-identity', 'kept');
  await expect(button).toBeFocused();
  await row.click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: 'Copy id' })).toBeVisible();
  await page.keyboard.press('Escape');
  await button.press('Enter');
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === 'agent_view' && call.args.session === 'notice-one'))).toBe(true);
});

test("project running rows show status, metadata and recent order", async ({ page }) => {
  const project = { name: projectName, url: 'https://example.test/example/widget', machine_id: 'build-box', machine_name: 'build-box', folder: '/srv/project', cloned: true };
  const agent = (id: string, status: string, last: number) => ({ kind: 'agent', machine_id: 'build-box', machine_name: 'build-box', id, agent: 'codex', title: id, cwd: '/srv/project', status, doing: null, started_at: 1, last_activity: last, external: false, question: null });
  await openDesktop(page, false, { project_sessions: [], agent_sessions: [], center_home: { machines: [], projects: [project], running: [agent('Idle', 'idle', 1), agent('Waiting', 'asking', 2), agent('Starting', 'starting', 3)], needs_you: [] } });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  const lane = (title: string) => page.locator('.pj-board .bd-lane').filter({ has: page.getByRole('heading', { name: title }) });
  const starting = lane('Working').locator('.bd-card').filter({ hasText: 'Starting' });
  await expect(starting.getByRole('img', { name: 'Starting' })).toBeVisible();
  await expect(lane('Needs you').locator('.bd-card').filter({ hasText: 'Waiting' })).toContainText('needs you');
  await expect(lane('Ready to review').locator('.bd-card')).toHaveText(/Idle/);
  await starting.focus();
  await starting.evaluate(el => { (el as HTMLElement).dataset.identity = 'kept'; });
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'center_home').length);
  const before = await reads();
  await page.evaluate(() => window.__navigation.notify('home'));
  await expect.poll(reads).toBeGreaterThan(before);
  await expect(starting).toHaveAttribute('data-identity', 'kept');
  await expect(starting).toBeFocused();
  await page.locator('.pj-strip button').filter({ hasText: 'running' }).click();
  const rows = page.locator('#sec-project-agents .cc-line');
  await expect(rows).toHaveCount(3);
  await expect(rows.locator('button.t b')).toHaveText(['Starting', 'Waiting', 'Idle']);
  await expect(rows.nth(0).getByRole('img', { name: 'Starting' })).toBeVisible();
  await expect(rows.nth(1)).toContainText('needs you');
  await expect(rows.nth(2)).toContainText('build-box');
});
