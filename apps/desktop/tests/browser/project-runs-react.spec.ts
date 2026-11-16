import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("workflow run retains focus while details update", async ({ page }) => {
  const run = { id: 42, number: 42, workflow: 'Build', title: 'Example run', branch: 'main', event: 'push', actor: 'Example', state: 'failed', url: 'https://example.test/run/42', started_at: 1, finished_at: 2, progress: null, pull_requests: [], host: 'github.com' };
  const detail = { run, jobs: [{ id: 1, name: 'Test', state: 'failed', started_at: 1, finished_at: 2, steps: [{ number: 1, name: 'Run tests', state: 'failed', started_at: 1, finished_at: 2 }], url: 'https://example.test/log' }], log: { job: 'Test', step: 'Run tests', text: 'Example failure', truncated: false }, error: null, rate_limited_until: null };
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: 'build-box', machine_name: 'build-box', agents: ['codex'], workspace: null, home: '/srv', capabilities: [] }],
    agent_models: { pick: 'listed', current: 'alpha', models: [{ id: 'alpha', name: 'Alpha', description: null }] },
    agent_begin: { session: { machine_id: 'build-box', machine_name: 'build-box', id: 'start:fixer', agent: 'codex', cwd: '/srv/project', status: 'starting', external: false, question: null }, why: '' },
    center_project_runs: { runs: [run], checks: [], synced_at: null, error: null, rate_limited_until: null }, center_project_run: detail,
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "actions" } }), projectName);
  const header = page.locator('.rr-head').filter({ hasText: 'Build #42' });
  await expect(header).toBeVisible();
  await header.focus();
  await header.evaluate(el => { el.dataset.identity = 'kept'; });
  await header.press('Enter');
  await expect(page.locator('.pj-log pre')).toHaveText('Example failure');
  await expect(header).toHaveAttribute('data-identity', 'kept');
  await expect(header).toBeFocused();
  await expect(header).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByRole('button', { name: 'Fix this' })).toBeVisible();
  await header.click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: 'Full log' })).toBeVisible();
  // The menu fills the bar to edit the prompt first; the button starts at once.
  await page.getByRole('menuitem', { name: 'Fix this' }).click();
  await expect(page.getByRole('textbox', { name: 'Task for a coding agent' })).toHaveValue(/Build/);
  await page.getByRole('button', { name: 'Fix this' }).click();
  await page.getByRole('dialog', { name: 'Harness, model and thinking' }).locator('[data-agmo="alpha"]').click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'agent_begin').length)).toBe(1);
  const args = await page.evaluate(() => window.__navigation.calls().find(call => call.command === 'agent_begin')?.args);
  expect(args).toMatchObject({ agent: 'codex', model: 'alpha' });
  expect(String(args?.prompt)).toContain('Fix the failing Build run #42');
});

test("workflow filters retain host labels and show read errors", async ({ page }) => {
  const run = (id: number, state: string, host: string) => ({ id, number: id, workflow: 'Build', title: `Run ${id}`, branch: 'main', event: 'push', actor: 'Example', state, url: `https://example.test/run/${id}`, started_at: 1, finished_at: 2, progress: null, pull_requests: [], host });
  const hosts = [{ host: 'github.com', kind: 'github', repo: 'example/widget', primary: true, synced_at: null, error: null, rate_limited_until: null }, { host: 'forge.example.test', kind: 'forgejo', repo: 'example/widget', primary: false, synced_at: null, error: 'Unavailable', rate_limited_until: null }];
  await openDesktop(page, false, { center_project_runs: { runs: [run(1, 'failed', 'github.com'), run(2, 'passed', 'forge.example.test')], checks: [], hosts, synced_at: null, error: 'Some runs could not be read', rate_limited_until: null } });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "actions" } }), projectName);
  const section = page.locator('#sec-actions');
  await expect(section).toContainText('Some runs could not be read');
  await expect(section).toContainText('Forgejo: Unavailable');
  await expect(section.locator('.pj-host')).toHaveText(['GitHub', 'Forgejo']);
  await section.getByRole('button', { name: 'failed 1' }).click();
  await expect(section.locator('.rr-head')).toHaveCount(1);
  await expect(section.locator('.rr-head')).toContainText('Build #1');
  await section.getByRole('button', { name: 'failed 1' }).click();
  await expect(section.locator('.rr-head')).toHaveCount(2);
});

test("showing the window again restarts the project's Actions", async ({ page }) => {
  const run = { id: 7, number: 7, workflow: 'Build', title: 'Example run', branch: 'main', event: 'push', actor: 'Example', state: 'passed', url: 'https://example.test/run/7', started_at: 1, finished_at: 2, progress: null, pull_requests: [], host: 'github.com' };
  await openDesktop(page, false, { center_project_runs: { runs: [run], checks: [], synced_at: null, error: null, rate_limited_until: null } });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "actions" } }), projectName);
  await expect(page.locator('#sec-actions')).toContainText('Build #7');
  const following = () => page.evaluate(() => {
    const sent = window.__navigation.calls().filter(call => call.command === 'mesh_watch' || call.command === 'mesh_topics').at(-1)?.args.topics;
    return Array.isArray(sent) && sent.some(interest => typeof interest?.topic === 'string' && interest.topic.startsWith('forge.') && interest.topic !== 'forge.*');
  });
  await expect.poll(following).toBe(true);
  await page.evaluate(() => window.__navigation.emit('center-hidden', null));
  await expect.poll(following).toBe(false);
  await page.evaluate(() => window.__navigation.emit('center-shown', null));
  await expect.poll(following).toBe(true);
  await expect(page.locator('#sec-actions')).toContainText('Build #7');
});

test("wheeling over an opened run's log scrolls the page", async ({ page }) => {
  const run = (id: number) => ({ id, number: id, workflow: 'Build', title: `Run ${id}`, branch: 'main', event: 'push', actor: 'Example', state: id === 40 ? 'failed' : 'passed', url: `https://example.test/run/${id}`, started_at: 1, finished_at: 2, progress: null, pull_requests: [], host: 'github.com' });
  const runs = Array.from({ length: 20 }, (_, i) => run(40 - i));
  const log = { job: 'Test', step: 'Run tests', text: Array.from({ length: 120 }, (_, i) => `line ${i}`).join("\n"), truncated: true };
  const detail = { run: runs[0], jobs: [{ id: 1, name: 'Test', state: 'failed', started_at: 1, finished_at: 2, steps: [{ number: 1, name: 'Run tests', state: 'failed', started_at: 1, finished_at: 2 }], url: 'https://example.test/log' }], log, error: null, rate_limited_until: null };
  await openDesktop(page, false, { center_project_runs: { runs, checks: [], synced_at: null, error: null, rate_limited_until: null }, center_project_run: detail });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "actions" } }), projectName);
  await page.locator('.rr-head').filter({ hasText: 'Build #40' }).click();
  const pre = page.locator('.pj-log pre'), scroller = page.locator('.pj-det').filter({ has: page.locator('#sec-actions') });
  await expect(pre).toContainText('line 119');
  const box = await pre.boundingBox();
  if (!box) throw new Error("the log is not laid out");
  await page.mouse.move(box.x + 40, box.y + 20);
  await page.mouse.wheel(0, 300);
  await expect.poll(() => scroller.evaluate(el => el.scrollTop)).toBeGreaterThan(0);
  expect(await pre.evaluate(el => el.scrollTop)).toBe(0);
});
