import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const group = { id: "group", name: "Example group", members: [], last: null, updated_at: 1, beep_typing: false, pinned: false, apps_shared: false };
const invite = { code: "ABC123", url: "https://example.test/invite", qr_svg: '<svg aria-label="Invite code"></svg>' };

test("group invite loads, copies and resets its code", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_group: { group, messages: [] }, group_typing: false, group_invite: invite });
  await page.evaluate(() => { Reflect.set(window, "copiedInvite", ""); Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text: string) => { Reflect.set(window, "copiedInvite", text); } } }); window.__navigation.open({ group: { id: "group" } }); });
  await page.locator("#ginvite").click();
  await expect(page.locator(".code.big")).toHaveText("ABC123");
  await expect(page.locator(".qr svg")).toBeVisible();
  await page.locator("#icode").click();
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "copiedInvite"))).toBe("ABC123");
  await page.locator("#ilink").click();
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "copiedInvite"))).toBe(invite.url);
  await page.evaluate(() => window.__navigation.respond("group_invite", { code: "NEW456", url: "https://example.test/new", qr_svg: '<svg aria-label="New code"></svg>' }));
  await page.locator("#ireset").click();
  await expect(page.locator(".code.big")).toHaveText("NEW456");
  await expect(page.getByText(/old one no longer works/)).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "group_invite").map(call => call.args))).toEqual([{ id: "group", reset: false }, { id: "group", reset: true }]);
});

test("late invite response cannot replace a different screen", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_group: { group, messages: [] }, group_typing: false, group_invite: invite });
  await page.evaluate(() => window.__navigation.open({ group: { id: "group" } }));
  await page.evaluate(() => window.__navigation.hold("group_invite"));
  await page.locator("#ginvite").click();
  await expect(page.getByText("Making a code…")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("#gask")).toBeVisible();
  await page.evaluate(() => window.__navigation.release("group_invite"));
  await expect(page.locator("#gask")).toBeVisible();
  await expect(page.locator(".code.big")).toHaveCount(0);
});

test("invite failure is shown and back returns to the group", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_group: { group, messages: [] }, group_typing: false });
  await page.evaluate(() => { window.__navigation.rejectOnce("group_invite"); window.__navigation.open({ group: { id: "group" } }); });
  await page.locator("#ginvite").click();
  await expect(page.locator(".derr")).toContainText("Requested test failure");
  await page.keyboard.press("Escape");
  await expect(page.locator("#gask")).toBeVisible();
});
