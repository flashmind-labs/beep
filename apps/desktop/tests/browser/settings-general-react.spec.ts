import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const responses = { status: { link: "unlinked", signed_out: true, server: false, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, mac_control: { id: "one", name: "build-box", enabled: false, status: "off", error: null, permissions: { screen_recording: false, accessibility: false } }, list_machines: [], skill_sync_get: true };
test("skill sync preserves state and serializes a save across Settings reopening", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const toggle = settings.getByRole("switch", { name: "Sync coding agent skills" });
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await page.evaluate(() => window.__navigation.hold("skill_sync_set"));
  await toggle.click();
  await expect(toggle).toBeDisabled();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "skill_sync_set" && call.args.enabled === false))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  await expect(toggle).toBeDisabled();
  await page.evaluate(() => window.__navigation.release("skill_sync_set"));
  await expect(toggle).toBeEnabled();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
});

test("general settings use their native commands and retain focus on refresh", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const toggle = settings.getByRole("switch", { name: "Coding agent notifications" });
  await toggle.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_agent_notices" && call.args.enabled === true))).toBe(true);
  await expect(toggle).toBeEnabled();
  const skill = settings.getByRole("switch", { name: "Sync coding agent skills" });
  await expect(skill).toBeEnabled();
  await skill.focus();
  await skill.evaluate(el => Reflect.set(window, "skillToggle", el));
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.__navigation.emit("link-state", "unlinked"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(reads);
  expect(await skill.evaluate(el => el === Reflect.get(window, "skillToggle") && document.activeElement === el)).toBe(true);
});

test("skill sync retries an unsuccessful read", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.evaluate(() => window.__navigation.rejectOnce("skill_sync_get"));
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const toggle = settings.getByRole("switch", { name: "Sync coding agent skills" });
  await expect(settings.getByRole("alert")).toContainText("Requested test failure");
  await expect(toggle).toBeDisabled();
  await settings.getByRole("button", { name: "Retry" }).click();
  await expect(toggle).toBeEnabled();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
});

test("voice, pause and autostart controls send the expected settings", async ({ page }) => {
  const current = { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false };
  await openDesktop(page, false, { ...responses, status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: current, list_devices: { devices: [], server: "answered" } });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const launch = settings.getByRole("switch", { name: "Launch at login" });
  await page.evaluate(value => window.__navigation.respond("settings", value), { ...current, autostart: true });
  await launch.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_autostart" && call.args.enabled === true))).toBe(true);
  await expect(launch).toHaveAttribute("aria-checked", "true");
  await page.evaluate(value => window.__navigation.respond("settings", value), { ...current, autostart: true, notice_voice: "ask" });
  await settings.getByRole("radio", { name: "Ask first" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_notice_voice" && call.args.choice === "ask"))).toBe(true);
  await expect(settings.getByRole("radio", { name: "Ask first" })).toHaveAttribute("aria-checked", "true");
  await page.evaluate(value => window.__navigation.respond("settings", value), { ...current, autostart: true, notice_voice: "ask", send_on_pause: true });
  await settings.getByRole("radio", { name: "Send when I stop talking" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_send_on_pause" && call.args.enabled === true))).toBe(true);
  await expect(settings.getByRole("radio", { name: "Send when I stop talking" })).toHaveAttribute("aria-checked", "true");
});

test("phone General hides launch at login", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.evaluate(() => { Reflect.set(window, "__BEEP_PHONE__", true); });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("switch", { name: "Sync coding agent skills" })).toBeVisible();
  await expect(settings.getByRole("switch", { name: "Launch at login" })).toHaveCount(0);
});
