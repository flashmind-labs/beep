import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("chat coding-agent card keeps its controls while answering a live question", async ({ page }) => {
  const id = 'chat-example';
  const report = 'codex session notice-one in /srv/project: asking, 1 turn\nDoing: Checking changes\nPlan:\n  [now] Inspect the files\nFiles changed: sample.ts\nQuestion 1: Continue?';
  const history = [{ role: 'user', text: 'Inspect the project', at: 1, images: [], model: '', thinking: '' }, { role: 'assistant', html: '<p>Working on it.</p>', tools: [{ name: 'code_agent', label: 'Start agent', success: true, route: null, file: null, images: [], products: null, cards: null, chart: null, code_agent: report }], replies: [], model: '' }];
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    let answered = false, reads = 0;
    window.addEventListener('test-answer', () => { answered = true; });
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === 'agent_view') { Reflect.set(window, 'cardReads', ++reads); return { session: { machine_id: 'build-box', machine_name: 'build-box', id: 'notice-one', agent: 'codex', cwd: '/srv/project', status: answered ? 'working' : 'asking', doing: 'Checking changes', external: false, question: answered ? null : { id: 1, text: 'Continue?', detail: null, options: [{ id: 'yes', name: 'Continue', allow: true }, { id: 'no', name: 'Stop', allow: false }] } }, plan: [{ content: 'Inspect the files', status: 'in_progress' }], files: ['sample.ts'], entries: [], earlier: 0, last_message: '', error: null, queued: 0, turns: 1 } as T; }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(session => window.__navigation.open({ chat: { session } }), id);
  const card = page.locator('.dcodercard');
  await expect(card.getByText('Continue?', { exact: true })).toBeVisible();
  await expect(card).toContainText('Inspect the files');
  await expect(card).toContainText('sample.ts');
  const open = card.getByRole('button', { name: 'Open', exact: true });
  await open.focus();
  await open.evaluate(el => { el.dataset.identity = 'kept'; });
  await page.evaluate(() => window.__navigation.hold('center_answer_agent'));
  await card.getByRole('button', { name: 'Continue', exact: true }).evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(card.getByRole('button', { name: 'Continue', exact: true })).toBeDisabled();
  await expect(open).toHaveAttribute('data-identity', 'kept');
  await expect(open).toBeFocused();
  await card.getByText('Inspect the files').evaluate(el => { const selection = window.getSelection(), range = document.createRange(); range.selectNodeContents(el); selection?.removeAllRanges(); selection?.addRange(range); });
  const selected = await page.evaluate(() => String(window.getSelection()));
  expect(selected).toBe('Inspect the files');
  const reads = await page.evaluate(() => Number(Reflect.get(window, 'cardReads')));
  await page.evaluate(() => { window.dispatchEvent(new Event('test-answer')); window.__navigation.release('center_answer_agent'); });
  await expect.poll(() => page.evaluate(() => Number(Reflect.get(window, 'cardReads')))).toBeGreaterThan(reads);
  await expect(card.getByText('Continue?', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => String(window.getSelection()))).toBe(selected);
  await page.evaluate(() => window.getSelection()?.removeAllRanges());
  await expect(card.getByText('Continue?', { exact: true })).toHaveCount(0);
  await expect(card).toContainText('Working');
  await expect(open).toHaveAttribute('data-identity', 'kept');
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'center_answer_agent').map(call => call.args))).toEqual([{ machineId: 'build-box', agent: 'codex', session: 'notice-one', question: 1, reply: 'yes' }]);
  await open.click();
  await expect(page.getByRole('tab', { name: 'Files', exact: true })).toBeVisible();
});

test("reopening a live chat resumes its coding-agent card", async ({ page }) => {
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    let reads = 0, doing = 'First agent activity';
    window.addEventListener('test-agent-updated', () => { doing = 'New agent activity'; });
    window.addEventListener('test-agent-latest', () => { doing = 'Latest agent activity'; });
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === 'send_prompt' && args?.onEvent && typeof args.onEvent === 'object' && 'onmessage' in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, 'sendChatEvent', (event: unknown) => { if ('onmessage' in channel && typeof channel.onmessage === 'function') channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      if (command === 'agent_view') {
        Reflect.set(window, 'liveCardReads', ++reads);
        const captured = doing;
        if (Reflect.get(window, 'holdCardRead')) { Reflect.set(window, 'holdCardRead', false); await new Promise<void>(resolve => Reflect.set(window, 'resumeCardRead', resolve)); }
        return { session: { machine_id: 'build-box', machine_name: 'build-box', id: 'notice-one', agent: 'codex', cwd: '/srv/project', status: 'working', doing: captured, external: false, question: null }, plan: [], files: [], entries: [], earlier: 0, last_message: '', error: null, queued: 0, turns: 1 } as T;
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open({ chat: { session: 'live-chat' } }));
  const prompt = page.locator('#ask');
  await prompt.fill('Start an agent');
  await prompt.press('Enter');
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, 'sendChatEvent'))).toBe('function');
  await page.evaluate(() => Reflect.get(window, 'sendChatEvent')({ kind: 'tool_result', call_id: 'call', name: 'code_agent', success: true, file: null, images: [], products: null, cards: null, chart: null, code_agent: 'codex session notice-one in /srv/project: working, 1 turn' }));
  const card = page.locator('.dcodercard');
  await expect(card).toContainText('First agent activity');
  const reads = await page.evaluate(() => Number(Reflect.get(window, 'liveCardReads')));
  await page.evaluate(() => window.__navigation.open('chats'));
  await expect(page.getByPlaceholder('Search chats and groups')).toBeVisible();
  await page.evaluate(() => { window.dispatchEvent(new Event('test-agent-updated')); window.__navigation.open({ chat: { session: 'live-chat' } }); });
  await expect(card).toContainText('New agent activity');
  await expect.poll(() => page.evaluate(() => Number(Reflect.get(window, 'liveCardReads')))).toBe(reads + 1);
  await page.evaluate(() => window.__navigation.open('chats'));
  await expect(page.getByPlaceholder('Search chats and groups')).toBeVisible();
  await page.evaluate(() => window.__navigation.open({ chat: { session: 'live-chat' } }));
  await expect.poll(() => page.evaluate(() => Number(Reflect.get(window, 'liveCardReads')))).toBe(reads + 2);
  await page.evaluate(() => { Reflect.set(window, 'holdCardRead', true); window.__navigation.open('chats'); });
  await expect(page.getByPlaceholder('Search chats and groups')).toBeVisible();
  await page.evaluate(() => window.__navigation.open({ chat: { session: 'live-chat' } }));
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, 'resumeCardRead'))).toBe('function');
  await page.evaluate(() => { window.dispatchEvent(new Event('test-agent-latest')); window.__navigation.open('chats'); });
  await expect(page.getByPlaceholder('Search chats and groups')).toBeVisible();
  await page.evaluate(() => window.__navigation.open({ chat: { session: 'live-chat' } }));
  await expect(card).toContainText('Latest agent activity');
  await page.evaluate(async () => { Reflect.get(window, 'resumeCardRead')(); await new Promise(resolve => setTimeout(resolve, 0)); });
  await expect(card).toContainText('Latest agent activity');
});

test("a coding-agent card reads its agent's change without an ambient wait", async ({ page }) => {
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: [{ role: 'assistant', html: '<p>Working on it.</p>', tools: [{ name: 'code_agent', label: 'Start agent', success: true, route: null, file: null, images: [], products: null, cards: null, chart: null, code_agent: 'codex session notice-one in /srv/project: working, 1 turn' }], replies: [], model: '' }], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: 'chat-example' } }));
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'agent_view').length);
  await expect.poll(reads).toBeGreaterThan(0);
  await page.waitForTimeout(300);
  const before = await reads();
  await page.evaluate(() => window.__navigation.publish('agent.notice-one', null));
  await expect.poll(reads, { timeout: 600 }).toBeGreaterThan(before);
});
