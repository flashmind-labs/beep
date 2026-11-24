import { expect, test } from "@playwright/test";
import { allowSelection, openDesktop, projectName } from "./harness.ts";

test("past sessions retain focused rows and scroll on refresh", async ({ page }) => {
  const sessions = Array.from({ length: 20 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100 - i, external: false, question: null }));
  await openDesktop(page, false, { project_sessions: sessions });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
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
  const tab = page.getByRole('tab', { name: /Sessions/ });
  await tab.focus();
  await tab.evaluate(el => { (el as HTMLElement).dataset.identity = 'kept'; (el as HTMLElement).click(); });
  await expect(tab).toHaveAttribute('data-identity', 'kept');
  await expect(tab).toBeFocused();
  await expect(row).toHaveAttribute('data-identity', 'kept');
  await row.scrollIntoViewIfNeeded();
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await row.click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: 'Copy id' })).toBeVisible();
  await page.keyboard.press('Escape');
  await row.press('Enter');
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === 'agent_view' && call.args.session === 'past-10'))).toBe(true);
});

test("large past session lists virtualize while retaining the focused row", async ({ page }) => {
  const sessions = Array.from({ length: 600 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 1000 - i, external: false, question: null }));
  await openDesktop(page, false, { project_sessions: sessions });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  const detail = page.locator('.pj-det');
  const rows = detail.locator('.pv-row');
  await expect(rows.first()).toBeVisible();
  await expect.poll(() => rows.count()).toBeLessThan(100);
  const focused = rows.first();
  await focused.focus();
  for (let i = 1; i <= 25; i++) {
    await page.keyboard.press('Tab');
    await expect(detail.locator(`.pv-row[data-index="${i}"]`)).toBeFocused();
  }
  await detail.evaluate(el => { el.scrollTop = 0; });
  await expect(detail.locator('.pv-row[data-index="0"]')).toBeVisible();
  await detail.locator('.pv-row[data-index="0"]').focus();
  await focused.evaluate(el => { (el as HTMLElement).dataset.identity = 'kept'; });
  await detail.evaluate(el => { el.scrollTop = el.scrollHeight; });
  await expect.poll(() => rows.count()).toBeLessThan(100);
  await expect(focused).toHaveAttribute('data-identity', 'kept');
  await expect(focused).toBeFocused();
  await expect(detail.getByText('Past session 599', { exact: true })).toBeVisible();
  await detail.evaluate(el => { el.scrollTop = 0; });
  await allowSelection(page);
  await focused.evaluate(el => {
    (document.activeElement as HTMLElement | null)?.blur();
    const range = document.createRange(), selection = document.getSelection();
    const text = el.querySelector<HTMLElement>('.t')!;
    range.selectNodeContents(text);
    selection?.removeAllRanges(); selection?.addRange(range);
  });
  await expect.poll(() => focused.evaluate(el => document.getSelection()?.containsNode(el, true))).toBe(true);
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await detail.evaluate(el => { el.scrollTop = el.scrollHeight; });
  await expect(focused).toHaveAttribute('data-identity', 'kept');
  await expect.poll(() => focused.evaluate(el => document.getSelection()?.containsNode(el, true))).toBe(true);
  await page.evaluate(() => document.getSelection()?.removeAllRanges());
  await page.locator('.pj-list .pj-li[title="Git"]').click();
  await page.locator('.gh-back').click();
  await page.locator('.pj-strip button').filter({ hasText: 'Sessions' }).click();
  await expect(detail.locator('.pv-row').first()).toBeVisible();
  await detail.evaluate(el => { el.scrollTop = el.scrollHeight; });
  await expect(detail.getByText('Past session 599', { exact: true })).toBeVisible();
});

test("newer sessions keep the visible session anchored", async ({ page }) => {
  const sessions = Array.from({ length: 600 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 1000 - i, external: false, question: null }));
  await openDesktop(page, false, { project_sessions: sessions });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  const detail = page.locator('.pj-det');
  await expect(detail.locator('.pv-row').first()).toBeVisible();
  await detail.evaluate(el => { el.scrollTop = 10000; });
  await expect.poll(() => detail.locator('.pv-row').first().getAttribute('data-index').then(Number)).toBeGreaterThan(0);
  const key = await detail.evaluate(el => [...el.querySelectorAll<HTMLElement>('.pv-row')].find(row => row.getBoundingClientRect().top >= el.getBoundingClientRect().top)?.dataset.key);
  expect(key).toBeTruthy();
  const anchor = detail.locator(`.pv-row[data-key="${key}"]`);
  await expect(anchor).toBeVisible();
  const before = await anchor.evaluate(el => el.getBoundingClientRect().top);
  const index = Number(await anchor.getAttribute('data-index'));
  await page.evaluate(({ rows, project }) => {
    window.__navigation.emit('session-list', { list: { project }, sessions: [{ ...rows[0], id: 'newer', title: 'Newer', last_activity: 2000 }, ...rows] });
  }, { rows: sessions, project: projectName });
  await expect(anchor).toHaveAttribute('data-index', String(index + 1));
  await expect(anchor).toBeVisible();
  await expect.poll(() => anchor.evaluate((el, top) => Math.abs(el.getBoundingClientRect().top - top), before)).toBeLessThan(2);
});


test("closed sessions appear in project history as soon as the machine reports ended", async ({ page }) => {
  const session = { machine_id: "build-box", machine_name: "build-box", id: "closing-session", agent: "codex", title: "Recently closed", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: Date.now(), external: false, question: null };
  await openDesktop(page, false, { project_sessions: [session], agent_sessions: [session] });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "running" }).click();
  const row = page.locator('#sec-project-agents .cc-line').filter({ hasText: session.title });
  await row.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Close session", exact: true }).click();
  await expect(row).toHaveCount(0);

  await page.evaluate(({ ended, project }) => {
    window.__navigation.respond("agent_sessions", [ended]);
    window.__navigation.notify("home", [{ command: "agent_sessions", value: [ended] }]);
    window.__navigation.emit("session-list", { list: { project }, sessions: [ended] });
  }, { ended: { ...session, status: "ended" }, project: projectName });
  await page.locator('.pj-list .pj-li[title="Sessions"]').click();
  await expect(page.locator('.pj-det .pv-row').filter({ hasText: session.title })).toBeVisible();
  await expect(page.locator('.pj-list .pj-li[title="Sessions"]')).toContainText("1 past");
});

test("a saved title shows in the project's sessions from the event", async ({ page }) => {
  const row = { machine_id: "build-box", machine_name: "build-box", id: "past-1", agent: "codex", title: "First session", cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100, external: false, question: null };
  await openDesktop(page, false, { project_sessions: [row] });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  await expect(page.getByText("First session", { exact: true })).toBeVisible();
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sessions").length);
  const before = await reads();

  await page.evaluate(() => window.__navigation.emit("agent-title-changed", { machine_id: "build-box", agent: "codex", session: "past-1", title: "Renamed session" }));
  await expect(page.getByText("Renamed session", { exact: true })).toBeVisible();
  expect(await reads()).toBe(before);
});
