import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";
import { SETTING_ROWS } from "../../src/stores/settings/settings-nav.ts";

test("Settings navigation survives refresh, supports subpages and cleans up", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false }, list_machines: [], list_credentials: [], list_github_accounts: [] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  await expect(nav).toBeVisible();
  await expect(settings.locator(".dset-nav")).toHaveCount(1);
  await expect(settings.locator(".dset-body")).toHaveCount(1);
  const agents = nav.getByRole("button", { name: "Agents", exact: true });
  await agents.click();
  await agents.evaluate(el => { const nav = el.closest("nav"); if (nav) { nav.style.maxHeight = "100px"; nav.scrollTop = 35; } Reflect.set(window, "settingsNavButton", el); Reflect.set(window, "settingsNav", nav); });
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(1);
  expect(await agents.evaluate(el => el === Reflect.get(window, "settingsNavButton") && document.activeElement === el)).toBe(true);
  expect(await nav.evaluate(el => el.scrollTop)).toBe(35);
  await settings.getByRole("tab", { name: "Configuration", exact: true }).click();
  await settings.locator('#set-agent-overview [data-sub="agent:codex"]').click();
  await expect(settings.getByRole("button", { name: "Back to Agents" })).toBeVisible();
  await expect(nav.getByRole("button", { name: "Agents", exact: true })).toHaveAttribute("aria-current", "true");
  await settings.getByRole("button", { name: "Back to Agents" }).click();
  await expect(nav.getByRole("button", { name: "Agents", exact: true })).toHaveAttribute("aria-current", "true");
  await expect(settings.getByRole("tab", { name: "Configuration", exact: true })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => { const nav: unknown = Reflect.get(window, "settingsNav"); return nav instanceof HTMLElement && !nav.isConnected; })).toBe(true);
});

test("Escape returns from a nested page before closing Settings", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false }, list_machines: [] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  await expect(nav.getByRole("button", { name: "Agents", exact: true })).toBeVisible();
  await nav.getByRole("button", { name: "Agents", exact: true }).click();
  await settings.getByRole("tab", { name: "Configuration", exact: true }).click();
  await settings.locator('#set-agent-overview [data-sub="agent:codex"]').click();
  await expect(settings.getByRole("button", { name: "Back to Agents" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(settings).toBeVisible();
  await expect(nav.getByRole("button", { name: "Agents", exact: true })).toHaveAttribute("aria-current", "true");
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
});

test("Settings load failure replaces the placeholder and can recover in the same dialog", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false }, list_machines: [], dictation_settings_get: { enabled: false, model: null } });
  await page.evaluate(() => window.__navigation.rejectOnce("settings"));
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.locator(".derr")).toContainText("Requested test failure");
  await expect(settings.locator(".dset-nav")).toHaveCount(0);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(settings.getByRole("navigation", { name: "Settings pages" })).toBeVisible();
  await expect(settings.locator(".derr")).toHaveCount(0);
  await expect(settings.locator(".dset-nav")).toHaveCount(1);
});


test("closing Settings during its initial read disposes the frame and ignores the late reply", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false }, list_machines: [] });
  await page.evaluate(() => window.__navigation.holdNext("settings"));
  const button = page.locator("#side").getByRole("button", { name: "Settings", exact: true });
  await button.click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.locator(".cc-skel")).toHaveCount(4);
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await expect(button).toBeFocused();
  await page.evaluate(() => window.__navigation.release("settings"));
  await expect(settings).toHaveCount(0);
  await button.click();
  await expect(settings.getByRole("navigation", { name: "Settings pages" })).toBeVisible();
  await expect(settings.locator(".cc-skel")).toHaveCount(0);
});

test("Settings search opens the first page it finds on Enter", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false }, list_machines: [] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  const search = nav.getByRole("searchbox", { name: "Search settings" });
  await expect(search).toBeFocused();
  await search.fill("skil");
  await expect(nav.getByRole("button")).toHaveText(["Skills", /^Sync coding agent skills/]);
  await page.keyboard.press("Enter");
  await expect(nav.getByRole("button", { name: "Skills", exact: true })).toHaveAttribute("aria-current", "true");
  await expect(search).toHaveValue("");
  await nav.getByRole("button", { name: "Skills", exact: true }).focus();
  await page.keyboard.press("/");
  await expect(search).toBeFocused();
  await search.fill("codex");
  await nav.getByRole("button", { name: /Codex/ }).click();
  await expect(settings.getByRole("button", { name: "Back to Agents" })).toBeVisible();
});

test("Settings search points at a row, and every row it finds is on its page", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false }, list_machines: [] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  const search = nav.getByRole("searchbox", { name: "Search settings" });
  await search.fill("developer");
  await page.keyboard.press("Enter");
  await expect(settings.locator(".dtoggle.pointed")).toContainText("Developer mode");
  await expect(settings.getByRole("switch", { name: "Developer mode" })).toBeFocused();
  for (const [where, title] of SETTING_ROWS) {
    const button = nav.locator(`[data-goto="${where}"]`);
    if (!await button.count()) continue;
    await button.click();
    await expect(settings.locator(".dset-page .dtoggle > span > b", { hasText: new RegExp(`^${title}$`) }), `${title} under ${where}`).toHaveCount(1);
  }
});
