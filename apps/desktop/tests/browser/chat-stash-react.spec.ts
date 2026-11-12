import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("stash panel retains row focus and scroll on refresh and pops a draft", async ({ page }) => {
  const entries = Array.from({ length: 12 }, (_, i) => ({ id: `entry-${i}`, text: `Saved draft ${i}`, images: [], files: i === 1 ? ['example.txt'] : [] }));
  await openDesktop(page, false, { agent_hosts: [{ machine_id: 'build-box', machine_name: 'build-box', agents: ['codex'], workspace: null, home: '/srv', capabilities: [] }], prompt_stash_list: entries, prompt_stash_read: { text: 'Restored draft', images: [], files: [], paths: [] } });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const field = page.getByRole('textbox', { name: 'Task for a coding agent' });
  await field.fill('Current draft');
  const stash = page.getByRole('button', { name: 'Stash 12' });
  await stash.evaluate(el => Reflect.set(window, 'stashButton', el));
  await stash.click();
  const panel = page.getByRole('dialog', { name: 'Project stash' });
  const items = panel.locator('.prompt-stash-items');
  await expect(panel).toBeVisible();
  await items.evaluate(el => { el.style.height = '100px'; el.style.overflow = 'auto'; el.scrollTop = 150; });
  const article = panel.locator('article').filter({ hasText: 'Saved draft 1' }).first();
  const remove = article.getByRole('button', { name: 'Remove' });
  await remove.focus();
  await remove.evaluate(el => { el.dataset.identity = 'kept'; });
  const top = await items.evaluate(el => el.scrollTop);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'prompt_stash_list').length);
  await page.evaluate(project => window.__navigation.emit('prompt-stash-changed', project), projectName);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'prompt_stash_list').length)).toBeGreaterThan(reads);
  await expect(remove).toHaveAttribute('data-identity', 'kept');
  expect(await stash.evaluate(el => el === Reflect.get(window, 'stashButton'))).toBe(true);
  await expect(remove).toBeFocused();
  await expect(items).toHaveJSProperty('scrollTop', top);
  await article.getByRole('button', { name: 'Stash draft & pop' }).click();
  await expect(panel).toHaveCount(0);
  await expect(field).toHaveValue('Restored draft');
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === 'prompt_stash_take' && call.args.id === 'entry-1'))).toBe(true);
});

test("stash Escape returns focus and Remove sends the selected id", async ({ page }) => {
  const entries = [{ id: 'entry', text: 'Saved message', images: [], files: [] }];
  await openDesktop(page, false, { agent_hosts: [{ machine_id: 'build-box', machine_name: 'build-box', agents: ['codex'], workspace: null, home: '/srv', capabilities: [] }], prompt_stash_list: entries });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const field = page.getByRole('textbox', { name: 'Task for a coding agent' });
  const open = page.getByRole('button', { name: 'Stash 1' });
  await open.click();
  const panel = page.getByRole('dialog', { name: 'Project stash' });
  // The panel sits above its button, their right edges together, and follows it.
  const sits = async () => {
    const button = (await open.boundingBox())!, box = (await panel.boundingBox())!;
    expect(box.y + box.height).toBeLessThanOrEqual(button.y - 7);
    expect(Math.abs(box.x + box.width - (button.x + button.width))).toBeLessThanOrEqual(1);
  };
  await sits();
  await page.setViewportSize({ width: 1100, height: 760 });
  await expect.poll(async () => (await panel.boundingBox())!.y + (await panel.boundingBox())!.height <= (await open.boundingBox())!.y - 7).toBe(true);
  await sits();
  await panel.getByRole('button', { name: 'Remove' }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === 'prompt_stash_remove' && call.args.id === 'entry'))).toBe(true);
  await page.keyboard.press('Escape');
  await expect(panel).toHaveCount(0);
  await expect(field).toBeFocused();
  await expect(open).toHaveAttribute('aria-expanded', 'false');
});

test("a pending pop does not close another project's stash", async ({ page }) => {
  const first = { name: projectName, url: 'https://example.test/example/widget', machine_id: 'build-box', machine_name: 'build-box', folder: '/srv/project', cloned: true, imported_at: 1 };
  const second = { ...first, name: 'example/other', folder: '/srv/other' };
  const entries = [{ id: 'entry', text: 'Saved message', images: [], files: [] }];
  await openDesktop(page, false, { center_home: { machines: [], projects: [first, second], running: [], needs_you: [] }, agent_hosts: [{ machine_id: 'build-box', machine_name: 'build-box', agents: ['codex'], workspace: null, home: '/srv', capabilities: [] }], prompt_stash_list: entries, prompt_stash_read: { text: 'Restored draft', images: [], files: [], paths: [] } });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), first.name);
  await page.getByRole('textbox', { name: 'Task for a coding agent' }).fill('Original draft');
  await page.getByRole('button', { name: 'Stash 1' }).click();
  await page.evaluate(() => window.__navigation.hold('prompt_stash_take'));
  await page.getByRole('dialog', { name: 'Project stash' }).getByRole('button', { name: 'Stash draft & pop' }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'prompt_stash_take').length)).toBe(1);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const result = await invoke<T>(command, args);
      if (command === 'agent_view' && result && typeof result === 'object' && 'session' in result) {
        const value = result as { session: { cwd: string } };
        value.session.cwd = '/srv/other';
      }
      return result;
    };
    window.__navigation.open({ agent: { kind: 'agent', machine_id: 'build-box', machine_name: 'build-box', id: 'notice-two', agent: 'codex', title: 'Other session', cwd: '/srv/other', status: 'working', doing: null, started_at: 1, last_activity: 1, external: false } });
  });
  await page.locator('[data-stash-open="example/other"]:visible').click();
  const panel = page.getByRole('dialog', { name: 'Project stash' });
  await expect(panel).toContainText(second.name);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'prompt_stash_list').length);
  await page.evaluate(() => window.__navigation.release('prompt_stash_take'));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'prompt_stash_list').length)).toBeGreaterThan(reads);
  await expect(panel).toBeVisible();
  await expect(panel).toContainText(second.name);
});

test("stash notice stays mounted through a composer redraw", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }], prompt_stash_list: [] });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const field = page.getByRole("textbox", { name: "Task for a coding agent" });
  await field.fill("Message to save");
  await page.getByText("Show more", { exact: true }).click();
  await page.getByRole("button", { name: "Stash draft" }).click();
  const note = page.locator(".prompt-stash-note");
  await expect(note).toHaveText("Message stashed");
  await note.evaluate(el => Reflect.set(window, "stashNotice", el));
  await page.evaluate(() => window.__navigation.notify("machines"));
  await expect(note).toHaveText("Message stashed");
  expect(await note.evaluate(el => el === Reflect.get(window, "stashNotice"))).toBe(true);
});

test("stash panel closes on an outside click and its indicator toggles it", async ({ page }) => {
  const entries = [{ id: "entry", text: "Saved message", images: [], files: [] }];
  await openDesktop(page, false, { agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }], prompt_stash_list: entries });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const open = page.getByRole("button", { name: "Stash 1" });
  const panel = page.getByRole("dialog", { name: "Project stash" });
  await open.click();
  await expect(panel).toBeVisible();
  await expect(panel.getByRole("button", { name: "Close stash" })).toBeFocused();
  await expect(open).toHaveAttribute("aria-expanded", "true");

  await panel.locator("p").click();
  await expect(panel).toBeVisible();

  await page.mouse.click(4, 4);
  await expect(panel).toHaveCount(0);
  await expect(open).toHaveAttribute("aria-expanded", "false");

  await open.click();
  await expect(panel).toBeVisible();
  await open.click();
  await expect(panel).toHaveCount(0);

  await open.click();
  await panel.getByRole("button", { name: "Close stash" }).click();
  await expect(panel).toHaveCount(0);
});
