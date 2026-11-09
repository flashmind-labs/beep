import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = (id: string, updated: number) => ({ id, title: `Chat ${id}`, pinned: false, updated_at: updated });
test("chat list keeps its selected row and scroll on refresh", async ({ page }) => {
  const chats = Array.from({ length: 20 }, (_, i) => session(String(i), 100 - i));
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, list_sessions: chats, list_groups: [] });
  await page.evaluate(() => window.__navigation.open('chats'));
  const search = page.getByPlaceholder('Search chats and groups');
  await expect(search).toBeVisible();
  const list = page.getByRole('listbox', { name: 'Chats' });
  await expect(list.getByRole('option')).toHaveCount(20);
  await search.press('ArrowDown');
  const chosen = list.getByRole('option', { selected: true });
  await expect(chosen).toContainText('Chat 1');
  await chosen.evaluate(el => { el.dataset.identity = 'kept'; });
  await list.evaluate(el => { el.style.height = '100px'; el.style.overflow = 'auto'; el.scrollTop = 200; });
  const top = await list.evaluate(el => el.scrollTop);
  await page.evaluate(rows => { window.__navigation.respond('list_sessions', [{ ...rows[19], updated_at: 1000 }, ...rows.slice(0, 19)]); window.__navigation.emit('sessions-changed', null); }, chats);
  await expect(list.getByRole('option').first()).toContainText('Chat 19');
  await expect(chosen).toContainText('Chat 1');
  await expect(chosen).toHaveAttribute('data-identity', 'kept');
  await expect(list).toHaveJSProperty('scrollTop', top);
  await expect(search).toBeFocused();
});

test("chat rename keeps a draft during refresh and sends it", async ({ page }) => {
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, list_sessions: [session('one', 1)], list_groups: [] });
  await page.evaluate(() => window.__navigation.open('chats'));
  const search = page.getByPlaceholder('Search chats and groups');
  await expect(page.getByRole('option', { name: /Chat one/ })).toBeVisible();
  await search.press('Tab');
  await page.getByRole('button', { name: /Rename/ }).click();
  const rename = page.locator('#rename');
  await rename.fill('Updated title');
  await page.evaluate(() => window.__navigation.emit('sessions-changed', null));
  await expect(rename).toHaveValue('Updated title');
  await expect(rename).toBeFocused();
  await rename.press('Enter');
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === 'rename_session' && call.args.title === 'Updated title'))).toBe(true);
});

test("rename keeps focus and caret when another chat moves above it", async ({ page }) => {
  const one = session('one', 20), two = session('two', 10);
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, list_sessions: [one, two], list_groups: [] });
  await page.evaluate(() => window.__navigation.open('chats'));
  const search = page.getByPlaceholder('Search chats and groups');
  await expect(page.getByRole('option', { name: /Chat one/ })).toBeVisible();
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'list_sessions').length);
  await page.evaluate(rows => {
    window.__navigation.respond('list_sessions', rows);
    const invoke = window.__TAURI__.core.invoke;
    let held = false;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === 'list_sessions' && !held) { held = true; await new Promise<void>(resolve => Reflect.set(window, 'resumeList', resolve)); }
      return invoke<T>(command, args);
    };
    window.__navigation.emit('sessions-changed', null);
  }, [one, { ...two, updated_at: 30 }]);
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, 'resumeList'))).toBe('function');
  await search.press('Tab');
  await page.getByRole('button', { name: /Rename/ }).click();
  const rename = page.locator('#rename');
  await rename.fill('Edited title');
  await rename.evaluate(el => { if (el instanceof HTMLInputElement) el.setSelectionRange(3, 3); });
  await page.evaluate(() => Reflect.get(window, 'resumeList')());
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'list_sessions').length)).toBeGreaterThan(reads);
  await expect(page.locator('#list > div').first()).toContainText('Chat two');
  await expect(rename).toHaveValue('Edited title');
  await expect(rename).toBeFocused();
  await expect(rename).toHaveJSProperty('selectionStart', 3);
});

test("chat share and unshare actions use the selected conversation", async ({ page }) => {
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, list_sessions: [session('one', 1)], list_groups: [], share_session: { url: 'https://example.test/share' } });
  await page.evaluate(() => window.__navigation.open('chats'));
  const search = page.getByPlaceholder('Search chats and groups');
  await expect(page.getByRole('option', { name: /Chat one/ })).toBeVisible();
  await search.press('Tab');
  await page.getByRole('button', { name: 'Share', exact: true }).click();
  await expect(page.locator('.dshare .code')).toHaveText('https://example.test/share');
  await page.getByRole('button', { name: 'Stop sharing' }).click();
  await expect(page.locator('.dshare')).toHaveCount(0);
  const calls = await page.evaluate(() => window.__navigation.calls().filter(call => ['share_session', 'unshare_session'].includes(call.command)));
  expect(calls.map(call => [call.command, call.args.id])).toEqual([['share_session', 'one'], ['unshare_session', 'one']]);
});

test("chat search preserves its field and caret when results arrive", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, list_sessions: [], list_groups: [], search_sessions: [] });
  await page.evaluate(() => window.__navigation.open("chats"));
  const field = page.getByPlaceholder("Search chats and groups");
  await field.fill("example");
  await field.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(2, 4); Reflect.set(window, "chatSearchField", el); } });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "search_sessions").length)).toBe(1);
  await expect(page.locator("#sec")).toHaveText("0 results");
  expect(await field.evaluate(el => el === Reflect.get(window, "chatSearchField") && document.activeElement === el && el instanceof HTMLInputElement && el.selectionStart === 2 && el.selectionEnd === 4)).toBe(true);
});

test("newer search wins over an earlier delayed result", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, list_sessions: [], list_groups: [], search_sessions: [] });
  await page.evaluate(() => window.__navigation.open("chats"));
  const field = page.getByPlaceholder("Search chats and groups");
  await page.evaluate(() => window.__navigation.holdNext("search_sessions"));
  await field.fill("first");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "search_sessions").length)).toBe(1);
  await page.evaluate(() => window.__navigation.respond("search_sessions", [{ session: { id: "new", title: "New result", pinned: false, updated_at: 2 }, snippet: "" }]));
  await field.fill("second");
  await expect(page.getByRole("option", { name: /New result/ })).toBeVisible();
  await page.evaluate(() => { window.__navigation.respond("search_sessions", [{ session: { id: "old", title: "Old result", pinned: false, updated_at: 1 }, snippet: "" }]); window.__navigation.release("search_sessions"); });
  await expect(page.getByRole("option", { name: /New result/ })).toBeVisible();
  await expect(page.getByRole("option", { name: /Old result/ })).toHaveCount(0);
  await expect(field).toBeFocused();
});

test("opening a group cancels a pending chat search", async ({ page }) => {
  const group = { id: "group", name: "Example group", members: [], last: null, updated_at: 1, beep_typing: false, pinned: false, apps_shared: false };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, list_sessions: [], list_groups: [group], open_group: { group, messages: [] }, group_typing: false });
  await page.evaluate(() => window.__navigation.open("chats"));
  const option = page.getByRole("option", { name: /Example group/ });
  await expect(option).toBeVisible();
  await page.getByPlaceholder("Search chats and groups").fill("group");
  await option.click();
  await expect(page.locator("#gask")).toBeVisible();
  await page.waitForTimeout(250);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "search_sessions").length)).toBe(0);
});
