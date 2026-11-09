import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const group = { id: "group", name: "Example group", members: [{ name: "Reader", you: true, creator: true, joined_at: 1 }, { name: "Writer", you: false, creator: false, joined_at: 2 }], last: null, updated_at: 1, beep_typing: false, pinned: false, apps_shared: false };

test("mention picker preserves composer focus and inserts with keyboard or pointer", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_group: { group, messages: [] }, group_typing: false });
  await page.evaluate(() => window.__navigation.open({ group: { id: "group" } }));
  const ask = page.locator("#gask");
  await expect(ask).toHaveAttribute("placeholder", /Example group/);
  const hints = page.locator("#bar .dhints");
  await hints.evaluate(el => Reflect.set(window, "groupHints", el));
  await ask.fill("Hello @");
  await expect(hints).toContainText("insert");
  expect(await hints.evaluate(el => el === Reflect.get(window, "groupHints"))).toBe(true);
  const picker = page.getByRole("listbox", { name: "Mention" });
  await expect(picker.getByRole("option")).toHaveCount(2);
  const composing = await ask.evaluate(el => el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true, isComposing: true })));
  expect(composing).toBe(true);
  await expect(picker.getByRole("option", { name: /Beep/ })).toHaveAttribute("aria-selected", "true");
  await ask.press("ArrowDown");
  await expect(picker.getByRole("option", { name: /Writer/ })).toHaveAttribute("aria-selected", "true");
  await ask.press("Enter");
  await expect(ask).toHaveValue("Hello @Writer ");
  await expect(ask).toBeFocused();
  await ask.fill("before @w after");
  await ask.evaluate(el => { if (el instanceof HTMLTextAreaElement) el.setSelectionRange(9, 9); });
  await ask.click({ position: { x: 10, y: 10 } });
  await ask.evaluate(el => { if (el instanceof HTMLTextAreaElement) { el.setSelectionRange(9, 9); el.dispatchEvent(new Event("click", { bubbles: true })); } });
  await expect(picker.getByRole("option", { name: /Writer/ })).toBeVisible();
  await ask.press("Enter");
  await expect(ask).toHaveValue("before @Writer  after");
  expect(await ask.evaluate(el => el instanceof HTMLTextAreaElement ? el.selectionStart : -1)).toBe(15);
  await ask.fill("@b");
  await picker.getByRole("option", { name: /Beep/ }).click();
  await expect(ask).toHaveValue("@beep ");
  await expect(ask).toBeFocused();
  await expect(picker).toHaveCount(0);
  await ask.fill("@");
  await expect(picker).toBeVisible();
  await page.evaluate(value => {
    window.__navigation.respond("open_group", { group: value, messages: [] });
    window.__navigation.emit("group-changed", "group");
  }, { ...group, members: [{ ...group.members[1], name: "New member" }] });
  await expect(picker.getByRole("option", { name: /New member/ })).toBeVisible();
  await expect(picker.getByRole("option", { name: /Writer/ })).toHaveCount(0);
  await ask.press("Escape");
  await expect(picker).toHaveCount(0);
  await ask.fill("@n");
  await expect(picker.getByRole("option", { name: /New member/ })).toBeVisible();
  await page.locator("#mention").evaluate(el => Reflect.set(window, "oldMentionHost", el));
  await page.evaluate(value => {
    window.__navigation.respond("open_group", { group: value, messages: [] });
    window.__navigation.open({ group: { id: value.id } });
  }, { ...group, id: "other", members: [{ ...group.members[1], name: "Other" }] });
  await expect.poll(() => page.evaluate(() => {
    const host: unknown = Reflect.get(window, "oldMentionHost");
    return host instanceof HTMLElement ? host.isConnected : true;
  })).toBe(false);
  await expect(ask).toHaveAttribute("placeholder", /Example group/);
  await ask.fill("@");
  await expect(picker.getByRole("option", { name: /Other/ })).toBeVisible();
  await expect(picker.getByRole("option", { name: /Writer/ })).toHaveCount(0);
});
