import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("Settings navigation survives refresh, supports subpages and cleans up", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_machines: [], list_credentials: [], list_github_accounts: [], agent_accounts: { claude: null, codex_key: null, machines: [] } });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  const agents = nav.getByRole("button", { name: "Agent accounts", exact: true });
  await agents.click();
  await agents.evaluate(el => { const nav = el.closest("nav"); if (nav) { nav.style.maxHeight = "100px"; nav.scrollTop = 35; } Reflect.set(window, "settingsNavButton", el); Reflect.set(window, "settingsNav", nav); });
  await page.evaluate(() => window.__navigation.emit("link-state", "linked"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(1);
  expect(await agents.evaluate(el => el === Reflect.get(window, "settingsNavButton") && document.activeElement === el)).toBe(true);
  expect(await nav.evaluate(el => el.scrollTop)).toBe(35);
  await nav.getByRole("button", { name: "Codex", exact: true }).click();
  await expect(settings.getByRole("button", { name: "Back to Agent accounts" })).toBeVisible();
  await settings.getByRole("button", { name: "Back to Agent accounts" }).click();
  await expect(nav.getByRole("button", { name: "Agent accounts", exact: true })).toHaveAttribute("aria-current", "true");
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => { const nav: unknown = Reflect.get(window, "settingsNav"); return nav instanceof HTMLElement && nav.parentElement === null; })).toBe(true);
});

test("Escape returns from a nested page before closing Settings", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "unlinked", signed_out: true, server: false, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_machines: [] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  await expect(nav.getByRole("button", { name: "Coding agents", exact: true })).toBeVisible();
  await expect(nav.getByRole("button", { name: "Agent accounts", exact: true })).toHaveCount(0);
  await expect(nav.getByRole("button", { name: "Beep server", exact: true })).toHaveCount(0);
  await nav.getByRole("button", { name: "Coding agents", exact: true }).click();
  await nav.getByRole("button", { name: "Codex", exact: true }).click();
  await expect(settings.getByRole("button", { name: "Back to Coding agents" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(settings).toBeVisible();
  await expect(nav.getByRole("button", { name: "Coding agents", exact: true })).toHaveAttribute("aria-current", "true");
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
});

test("delayed empty model list reconciles navigation safely", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_devices: { devices: [], server: "answered" }, get_models: { choices: [{ id: "one", name: "One", input_price: null, output_price: null, sees_images: false, default: true }], selected: "one", thinking: "", auto: false, auto_available: false }, list_machines: [], list_credentials: [], list_github_accounts: [], agent_accounts: { claude: null, codex_key: null, machines: [] } });
  await page.evaluate(() => { window.__navigation.respond("get_models", { choices: [], selected: "", thinking: "", auto: false, auto_available: false }); window.__navigation.holdNext("get_models"); });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  await expect(nav.getByRole("button", { name: "Models", exact: true })).toBeVisible();
  await page.evaluate(() => window.__navigation.release("get_models"));
  await expect(nav.getByRole("button", { name: "Models", exact: true })).toHaveCount(0);
  await nav.getByRole("button", { name: "Agent accounts", exact: true }).click();
  await nav.getByRole("button", { name: "General", exact: true }).click();
  await expect(nav.getByRole("button", { name: "General", exact: true })).toHaveAttribute("aria-current", "true");
  expect(errors).toEqual([]);
});
