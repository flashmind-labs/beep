import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("streamed replies retain selection and host identity until selection clears", async ({ page }) => {
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === 'send_prompt' && args?.onEvent && typeof args.onEvent === 'object' && 'onmessage' in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, 'sendChatEvent', (event: unknown) => { if (typeof channel.onmessage === 'function') channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open({ chat: { session: 'live-chat' } }));
  await page.locator('#ask').fill('Explain');
  await page.locator('#ask').press('Enter');
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, 'sendChatEvent'))).toBe('function');
  await page.evaluate(() => Reflect.get(window, 'sendChatEvent')({ kind: 'text', html: '<p>First reply</p>' }));
  const reply = page.locator('.dcoderen.agent');
  await expect(reply).toContainText('First reply');
  await reply.evaluate(el => {
    el.setAttribute('data-identity', 'kept');
    const range = document.createRange();
    range.selectNodeContents(el.querySelector('p')!);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  });
  await page.evaluate(() => Reflect.get(window, 'sendChatEvent')({ kind: 'text', html: '<p>Updated reply</p>' }));
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await expect(reply).toContainText('First reply');
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe('First reply');
  await page.evaluate(() => window.getSelection()?.removeAllRanges());
  await expect(reply).toContainText('Updated reply');
  await expect(reply).toHaveAttribute('data-identity', 'kept');
});

test("history refresh preserves selected reply and applies its update later", async ({ page }) => {
  const id = 'chat-example';
  const user = { role: 'user', text: 'Explain', at: 1, images: [], model: '', thinking: '' };
  const answer = { role: 'assistant', html: '<p>First answer</p>', tools: [], replies: [], model: '' };
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(session => window.__navigation.open({ chat: { session } }), id);
  const reply = page.locator('.dcoderen.agent');
  await expect(reply).toContainText('First answer');
  await reply.evaluate(el => {
    el.setAttribute('data-identity', 'kept');
    const range = document.createRange();
    range.selectNodeContents(el.querySelector('p')!);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
  });
  await page.evaluate(({ id, user, answer }) => {
    window.__navigation.respond('open_session', [user, { ...answer, html: '<p>Updated answer</p>' }]);
    window.__navigation.emit('sessions-changed', id);
  }, { id, user, answer });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'open_session').length)).toBeGreaterThan(1);
  await expect(reply).toContainText('First answer');
  await page.evaluate(() => window.getSelection()?.removeAllRanges());
  await expect(reply).toContainText('Updated answer');
  await expect(reply).toHaveAttribute('data-identity', 'kept');
});

test("history combines assistant fragments and keeps code copy controls", async ({ page }) => {
  const user = { role: 'user', text: 'Explain', at: 1, images: [], model: '', thinking: '' };
  const assistant = { role: 'assistant', tools: [], replies: [], model: '' };
  const history = [user, { ...assistant, html: '<p>First fragment</p>' }, { ...assistant, html: '<pre><code>example</code></pre>' }];
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: 'chat-example' } }));
  const reply = page.locator('.dcoderen.agent');
  await expect(reply.locator('p')).toHaveText('First fragment');
  await expect(reply.locator('pre code')).toHaveText('example');
  await expect(reply.locator('pre button[data-copy]')).toBeVisible();
});
