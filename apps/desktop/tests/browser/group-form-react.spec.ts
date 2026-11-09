import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

async function openForm(page: import("@playwright/test").Page) {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, list_groups: [], list_sessions: [] });
  await page.evaluate(() => window.__navigation.open("chats"));
  await page.keyboard.press(test.info().project.name === "macOS" ? "Meta+g" : "Control+g");
  await expect(page.locator("#jname")).toBeFocused();
}

test("group creation validates names and opens invite", async ({ page }) => {
  await openForm(page);
  await page.locator("#jgo").click();
  await expect(page.locator("#jname")).toBeFocused();
  await page.locator("#jname").fill("Example group");
  await page.locator("#jgo").click();
  await expect(page.locator("#jme")).toBeFocused();
  await page.locator("#jme").fill("Reader");
  await page.evaluate(() => { window.__navigation.respond("create_group", { id: "group", name: "Example group" }); window.__navigation.respond("group_invite", { code: "ABC123", url: "https://example.test", qr_svg: "<svg></svg>" }); });
  await page.locator("#jgo").click();
  await expect(page.locator(".code.big")).toHaveText("ABC123");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "create_group").map(call => call.args))).toEqual([{ name: "Example group", memberName: "Reader" }]);
});

test("joining a group follows the preview and sends its token", async ({ page }) => {
  await openForm(page);
  await page.locator("#jcode").click();
  await expect(page.locator("#jtoken")).toBeFocused();
  await page.evaluate(() => window.__navigation.respond("peek_group", { token: "ticket", preview: { id: "group", name: "Example group", members: ["Writer"], joined: false } }));
  await page.locator("#jtoken").fill("CODE");
  await page.locator("#jtoken").press("Enter");
  await expect(page.getByText("Join Example group?")).toBeVisible();
  await expect(page.locator("#jme")).toBeFocused();
  await page.locator("#jme").fill("Reader");
  await page.evaluate(() => { window.__navigation.respond("join_group", { id: "group" }); window.__navigation.respond("open_group", { group: { id: "group", name: "Example group", members: [], last: null, updated_at: 1, beep_typing: false, pinned: false, apps_shared: false }, messages: [] }); });
  await page.locator("#jgo").click();
  await expect(page.locator("#gask")).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "join_group").map(call => call.args))).toEqual([{ token: "ticket", memberName: "Reader" }]);
});

test("late code lookup cannot replace a new screen", async ({ page }) => {
  await openForm(page);
  await page.locator("#jcode").click();
  await page.evaluate(() => { window.__navigation.respond("peek_group", { token: "ticket", preview: { id: "group", name: "Example group", members: [], joined: false } }); window.__navigation.hold("peek_group"); });
  await page.locator("#jtoken").fill("CODE");
  await page.locator("#jgo").click();
  await page.keyboard.press("Escape");
  await expect(page.locator("#jname")).toBeVisible();
  await page.evaluate(() => window.__navigation.release("peek_group"));
  await expect(page.locator("#jname")).toBeVisible();
  await expect(page.getByText("Join Example group?")).toHaveCount(0);
});
