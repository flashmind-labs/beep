import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const group = { id: 'group', name: 'Example group', members: [{ name: 'Reader', you: true, creator: true, joined_at: 1 }], last: null, updated_at: 1, beep_typing: false, pinned: false, apps_shared: false };
const message = (seq: number) => ({ seq, author: { kind: 'member', name: 'Writer', you: false }, text: `Message ${seq}`, html: '', timestamp: 1, images: [] });

test("group refresh preserves selected messages, composer draft and scroll", async ({ page }) => {
  const messages = Array.from({ length: 25 }, (_, index) => index === 0 ? { ...message(1), images: [{ id: 'picture', mime: 'image/png', width: 1, height: 1 }] } : message(index + 1));
  await openDesktop(page, false, { picture: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=', status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_group: { group, messages }, group_typing: false });
  await page.evaluate(() => window.__navigation.open({ group: { id: 'group' } }));
  const body = page.locator('#gbody'), prompt = page.locator('#gask');
  const row = body.locator('.dg').filter({ hasText: 'Message 1' }).first();
  await expect(row).toBeVisible();
  const picture = row.locator('img.dpic');
  await expect(picture).toBeVisible();
  await picture.evaluate(el => { el.dataset.identity = 'kept'; });
  await prompt.fill('Unsent message');
  await body.evaluate(el => { el.style.height = '150px'; el.style.overflow = 'auto'; el.scrollTop = 40; });
  await row.evaluate(el => { el.dataset.identity = 'kept'; const text = el.querySelector('.b'); if (!text) return; const selection = window.getSelection(), range = document.createRange(); range.selectNodeContents(text); selection?.removeAllRanges(); selection?.addRange(range); });
  const top = await body.evaluate(el => el.scrollTop), selected = await page.evaluate(() => String(window.getSelection()));
  await page.evaluate(value => { window.__navigation.respond('open_group', value); window.__navigation.emit('group-changed', 'group'); }, { group, messages: [{ ...messages[0], text: "Updated message" }, ...messages.slice(1), message(26)] });
  await expect(body).toContainText('Message 26');
  await expect(row).toHaveAttribute('data-identity', 'kept');
  await expect(picture).toHaveAttribute('data-identity', 'kept');
  expect(selected).toBe('Message 1');
  await expect(body).toHaveJSProperty('scrollTop', top);
  expect(await page.evaluate(() => String(window.getSelection()))).toBe(selected);
  await expect(row).toContainText('Message 1');
  await expect(prompt).toHaveValue('Unsent message');
  await page.evaluate(() => window.getSelection()?.removeAllRanges());
  await expect(body.locator('.dg[data-identity=kept]')).toContainText('Updated message');
});

test("group approval sends the selected decision once", async ({ page }) => {
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_group: { group, messages: [] }, group_typing: false, list_apps: [] });
  await page.evaluate(() => window.__navigation.open({ group: { id: 'group' } }));
  await expect(page.locator('#gask')).toBeVisible();
  await page.evaluate(() => window.__navigation.emit('group-approval', { group_id: 'group', query_id: 'approval', app: 'Example', action: 'Read records', fields: [{ label: 'Scope', value: 'Example' }] }));
  const allow = page.getByRole('button', { name: 'Allow', exact: true });
  await expect(allow).toBeVisible();
  await allow.click();
  await expect(page.getByText('Allowed', { exact: true })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'answer_group_approval').map(call => call.args))).toEqual([{ queryId: 'approval', allow: true }]);
});
