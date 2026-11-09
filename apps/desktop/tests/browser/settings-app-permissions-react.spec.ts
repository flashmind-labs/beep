import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const app = { slug: "fastmail", name: "Fastmail", description: "Mail", logo_url: null, connected: true, configured: true, kind: "fastmail" };
const permissions = { level: "ask", actions: [{ tool: "send", label: "Send mail", allowed: false }] };
async function openApp(page: Page) {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_apps: [app], list_mcp_servers: [], app_permissions: permissions, list_app_accounts: [{ id: "one", address: "sample@example.test" }, { id: "two", address: "other@example.test" }] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: /^Apps/ }).click();
  await settings.locator("[data-open=fastmail]").click();
  await expect(settings.getByRole("switch", { name: "Always allow Send mail" })).toBeVisible();
  return settings;
}
test("permissions update in place and account removal requires confirmation", async ({ page }) => {
  const settings = await openApp(page);
  const toggle = settings.getByRole("switch", { name: "Always allow Send mail" });
  await toggle.focus();
  await toggle.evaluate(el => Reflect.set(window, "permissionToggle", el));
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  expect(await toggle.evaluate(el => el === Reflect.get(window, "permissionToggle") && document.activeElement === el)).toBe(true);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_app_permissions" && call.args.slug === "fastmail" && JSON.stringify(call.args.allowed) === JSON.stringify(["send"])))).toBe(true);
  const row = settings.locator(".daction").filter({ hasText: "sample@example.test" });
  await row.getByRole("button", { name: "Remove", exact: true }).click();
  await expect(row.getByText("Beep stops using this account")).toBeVisible();
  await row.getByRole("button", { name: "Remove", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "remove_app_account" && call.args.id === "one"))).toBe(true);
});

test("late permission updates do not affect a reopened app", async ({ page }) => {
  const settings = await openApp(page);
  await page.evaluate(() => window.__navigation.hold("set_app_permissions"));
  await settings.getByRole("switch", { name: "Always allow Send mail" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_app_permissions"))).toBe(true);
  await page.keyboard.press("Escape");
  await settings.locator("[data-open=fastmail]").click();
  const toggle = settings.getByRole("switch", { name: "Always allow Send mail" });
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await page.evaluate(() => window.__navigation.release("set_app_permissions"));
  await expect(toggle).toHaveAttribute("aria-checked", "false");
});

test("failed permission changes roll back and allow retry", async ({ page }) => {
  const settings = await openApp(page);
  await page.evaluate(() => window.__navigation.rejectOnce("set_app_permissions"));
  const toggle = settings.getByRole("switch", { name: "Always allow Send mail" });
  await toggle.click();
  await expect(settings.getByText("Error: Requested test failure")).toBeVisible();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(toggle).toBeEnabled();
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "set_app_permissions").length)).toBe(2);
});

test("permission writes stay serialized across reopening", async ({ page }) => {
  const settings = await openApp(page);
  await page.evaluate(() => window.__navigation.hold("set_app_permissions"));
  await settings.getByRole("switch", { name: "Always allow Send mail" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "set_app_permissions").length)).toBe(1);
  await page.keyboard.press("Escape");
  await settings.locator("[data-open=fastmail]").click();
  const toggle = settings.getByRole("switch", { name: "Always allow Send mail" });
  await expect(toggle).toBeDisabled();
  await page.evaluate(value => window.__navigation.respond("app_permissions", value), { ...permissions, actions: [{ ...permissions.actions[0], allowed: true }] });
  await page.evaluate(() => window.__navigation.release("set_app_permissions"));
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await expect(toggle).toBeEnabled();
  await toggle.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "set_app_permissions").length)).toBe(2);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "set_app_permissions").at(-1)?.args.allowed)).toEqual([]);
});

test("a stale read failure cannot replace a newer permissions refresh", async ({ page }) => {
  const settings = await openApp(page);
  await page.evaluate(() => window.__navigation.hold("set_app_permissions"));
  await settings.getByRole("switch", { name: "Always allow Send mail" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "set_app_permissions").length)).toBe(1);
  await page.keyboard.press("Escape");
  await page.evaluate(() => window.__navigation.holdNext("app_permissions"));
  await settings.locator("[data-open=fastmail]").click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "app_permissions").length)).toBe(2);
  await page.evaluate(value => window.__navigation.respond("app_permissions", value), { ...permissions, actions: [{ ...permissions.actions[0], allowed: true }] });
  await page.evaluate(() => window.__navigation.release("set_app_permissions"));
  const toggle = settings.getByRole("switch", { name: "Always allow Send mail" });
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await expect(toggle).toBeEnabled();
  await page.evaluate(() => { window.__navigation.rejectOnce("app_permissions"); window.__navigation.release("app_permissions"); });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "app_permissions").length)).toBe(3);
  await expect(settings.getByText("Error: Requested test failure")).toHaveCount(0);
  await expect(toggle).toHaveAttribute("aria-checked", "true");
});
