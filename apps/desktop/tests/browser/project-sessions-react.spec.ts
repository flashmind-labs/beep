import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("past sessions retain focused rows and scroll on refresh", async ({ page }) => {
  const sessions = Array.from({ length: 20 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100 - i, external: false, question: null }));
  await openDesktop(page, false, { project_sessions: sessions });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator('.pj-list .pj-li[title="Sessions"]').click();
  const row = page.locator('.pj-det .pv-row').filter({ hasText: 'Past session 10' });
  await expect(row).toBeVisible();
  const detail = page.locator('.pj-det');
  await detail.evaluate(el => { el.style.maxHeight = '120px'; el.style.overflow = 'auto'; el.scrollTop = el.scrollHeight; });
  await row.focus();
  await row.evaluate(el => { el.dataset.identity = 'kept'; });
  const top = await detail.evaluate(el => el.scrollTop);
  await page.getByRole('tab', { name: /Sessions/ }).evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(row).toHaveAttribute('data-identity', 'kept');
  await expect(row).toBeFocused();
  await expect(detail).toHaveJSProperty('scrollTop', top);
  await row.click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: 'Copy id' })).toBeVisible();
  await page.keyboard.press('Escape');
  await row.press('Enter');
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === 'agent_view' && call.args.session === 'past-10'))).toBe(true);
});

test("compact past sessions keep their height and scroll during refresh", async ({ page }) => {
  const sessions = Array.from({ length: 20 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100 - i, external: false, question: null }));
  await openDesktop(page, false, { project_sessions: sessions });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const list = page.locator('#sec-sessions .pj-scroll:visible');
  await expect(list.locator('.pv-row')).toHaveCount(20);
  await expect.poll(() => list.evaluate(el => (el as HTMLElement).style.maxHeight)).not.toBe('');
  await list.evaluate(el => { el.scrollTop = el.scrollHeight; });
  const before = await list.evaluate(el => el.scrollTop);
  expect(before).toBeGreaterThan(0);
  await page.locator('.pj-list .pj-li[title="Overview"]').evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(list).toHaveJSProperty('scrollTop', before);
  await expect.poll(() => list.evaluate(el => (el as HTMLElement).style.maxHeight)).not.toBe('');
});
