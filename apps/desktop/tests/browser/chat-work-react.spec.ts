import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("chat work keeps its heading and fold state on history refresh", async ({ page }) => {
  const id = 'chat-example';
  const history = [{ role: 'user', text: 'Explain this', at: 1, images: [], model: '', thinking: '' }, { role: 'assistant', html: '<p>Done.</p>', tools: [{ name: 'web_search', label: 'Search example', success: true, route: null, file: null, images: [], products: null, cards: null, chart: null }], replies: [], model: '' }];
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(session => window.__navigation.open({ chat: { session } }), id);
  const heading = page.locator('#body .tools .hd[data-turn]');
  await expect(heading).toBeVisible();
  await heading.focus();
  await heading.evaluate(el => { el.dataset.identity = 'kept'; });
  await heading.press('Enter');
  await expect(heading).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('#body .tools .tool')).toContainText('Search');
  await page.evaluate(session => window.__navigation.emit('sessions-changed', session), id);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'open_session').length)).toBeGreaterThan(1);
  await expect(heading).toHaveAttribute('data-identity', 'kept');
  await expect(heading).toHaveAttribute('aria-expanded', 'true');
  await expect(heading).toBeFocused();
});
