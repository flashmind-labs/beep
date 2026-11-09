import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const group = { id: "group", name: "Example group", members: [{ name: "Reader", you: true, creator: true, joined_at: 1 }, { name: "Writer", you: false, creator: false, joined_at: 2 }], last: null, updated_at: 1, beep_typing: false, pinned: false, apps_shared: false };
const reminder = { id: "one", label: "Review", message: "Check notes", schedule: "0 9 * * *", next_run_at: null, set_by: "Reader" };
async function openInfo(page: import("@playwright/test").Page) {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_group: { group, messages: [] }, group_typing: false, list_group_reminders: [reminder] });
  await page.evaluate(() => window.__navigation.open({ group: { id: "group" } }));
  await page.locator("#ginfo").click();
  await expect(page.getByText("Review", { exact: true })).toBeVisible();
}
test("group info retains controls and scroll while sharing changes", async ({ page }) => {
  await openInfo(page);
  const body = page.locator("#ginfobody"), share = page.getByRole("switch", { name: "Share my apps with Beep here" });
  await body.evaluate(el => { el.style.height = "100px"; el.style.maxHeight = "100px"; el.style.flex = "0 0 100px"; el.style.overflow = "auto"; el.scrollTop = 30; });
  await share.focus();
  await share.evaluate(el => Reflect.set(window, "groupShare", el));
  await page.evaluate(() => window.__navigation.hold("share_group_apps"));
  await share.click();
  await expect(share).toHaveAttribute("aria-disabled", "true");
  await body.evaluate(el => { el.scrollTop = 30; });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "share_group_apps").map(call => call.args))).toEqual([{ id: "group", shared: true }]);
  await page.evaluate(() => window.__navigation.release("share_group_apps"));
  await expect(share).toHaveAttribute("aria-checked", "true");
  expect(await share.evaluate(el => el === Reflect.get(window, "groupShare") && document.activeElement === el)).toBe(true);
  await expect(body).toHaveJSProperty("scrollTop", 30);
});

test("group reminder errors retry and leaving requires confirmation", async ({ page }) => {
  await openInfo(page);
  await page.evaluate(() => window.__navigation.rejectOnce("delete_group_reminder"));
  const remove = page.getByRole("button", { name: "Delete reminder" });
  await remove.click();
  await expect(page.locator("#ginfobody .derr")).toContainText("Requested test failure");
  await expect(remove).toHaveAttribute("aria-disabled", "false");
  await remove.click();
  await expect(page.getByText("Review", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Leave group", exact: true }).click();
  await expect(page.getByText(/Leave Example group\?/)).toBeVisible();
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(page.getByText(/Leave Example group\?/)).toHaveCount(0);
});

test("late leave does not navigate away from a different group", async ({ page }) => {
  await openInfo(page);
  await page.locator("#ginfobody").evaluate(el => Reflect.set(window, "oldGroupInfo", el));
  await page.evaluate(() => window.__navigation.hold("leave_group"));
  await page.getByRole("button", { name: "Leave group", exact: true }).click();
  await page.getByRole("button", { name: "Leave", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "leave_group"))).toBe(true);
  await page.evaluate(value => { window.__navigation.respond("open_group", { group: value, messages: [] }); window.__navigation.open({ group: { id: value.id } }); }, { ...group, id: "other", name: "Other group" });
  await expect(page.locator("#gask")).toHaveAttribute("placeholder", /Other group/);
  await expect.poll(() => page.evaluate(() => { const old: unknown = Reflect.get(window, "oldGroupInfo"); return old instanceof HTMLElement && old.isConnected; })).toBe(false);
  await page.evaluate(() => window.__navigation.release("leave_group"));
  await expect(page.locator("#gask")).toHaveAttribute("placeholder", /Other group/);
});

test("a stale group refresh cannot undo a completed sharing change", async ({ page }) => {
  await openInfo(page);
  await page.evaluate(() => { window.__navigation.holdNext("open_group"); window.__navigation.emit("group-changed", "group"); });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_group").length)).toBeGreaterThan(1);
  const share = page.getByRole("switch", { name: "Share my apps with Beep here" });
  await share.click();
  await expect(share).toHaveAttribute("aria-checked", "true");
  await page.evaluate(() => window.__navigation.release("open_group"));
  await expect(share).toHaveAttribute("aria-checked", "true");
});

test("a stale reminder read cannot restore a deleted reminder", async ({ page }) => {
  await openInfo(page);
  await page.evaluate(() => window.__navigation.hold("delete_group_reminder"));
  await page.getByRole("button", { name: "Delete reminder" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "delete_group_reminder"))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.locator("#gask")).toBeVisible();
  await page.evaluate(() => window.__navigation.holdNext("list_group_reminders"));
  await page.locator("#ginfo").click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "list_group_reminders").length)).toBe(2);
  await page.evaluate(() => { window.__navigation.respond("list_group_reminders", []); window.__navigation.release("delete_group_reminder"); });
  await expect(page.getByText("No reminders yet.", { exact: false })).toBeVisible();
  await page.evaluate(value => { window.__navigation.respond("list_group_reminders", [value]); window.__navigation.release("list_group_reminders"); }, reminder);
  await expect(page.getByText("Review", { exact: true })).toHaveCount(0);
});
