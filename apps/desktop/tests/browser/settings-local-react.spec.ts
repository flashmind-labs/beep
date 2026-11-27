import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const responses = { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false }, mac_control: { id: "one", name: "build-box", enabled: true, status: "connected", error: null, permissions: { screen_recording: false, accessibility: false } }, list_machines: [] };
test("Appearance keeps the chosen control focused across refresh", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Appearance", exact: true }).click();
  const skin = settings.getByRole("button", { name: "Nord", exact: true });
  await skin.click();
  await expect(skin).toHaveAttribute("aria-pressed", "true");
  await skin.evaluate(el => { Reflect.set(window, "chosenSkin", el); Reflect.set(window, "appearanceHost", el.closest("#set-appearance")); });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(reads);
  await expect(skin).toHaveAttribute("aria-pressed", "true");
  expect(await skin.evaluate(el => el === Reflect.get(window, "chosenSkin") && document.activeElement === el)).toBe(true);
  await page.evaluate(() => { localStorage.setItem("beep.skin", "quiet"); window.dispatchEvent(new StorageEvent("storage", { key: "beep.skin", newValue: "quiet" })); });
  await expect(settings.getByRole("button", { name: "Quiet", exact: true })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect.poll(() => page.evaluate(() => { const host: unknown = Reflect.get(window, "appearanceHost"); return host instanceof HTMLElement ? host.isConnected : null; })).toBe(false);
});

test("System theme adapts between chosen dark and light skins with system preferences", { tag: "@windows" }, async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Appearance", exact: true }).click();

  const toggle = settings.getByRole("switch", { name: "System theme" });
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(settings.locator('[data-skin-pick="linear"]')).toHaveAttribute("aria-pressed", "true");
  await expect(settings.locator('[data-skin-pick="paper"]')).toHaveAttribute("aria-pressed", "false");

  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await expect(settings).toContainText("Switches between Linear and Light with the system appearance");
  await expect(settings.locator('[data-skin-pick="linear"]')).toHaveAttribute("aria-pressed", "true");
  await expect(settings.locator('[data-skin-pick="paper"]')).toHaveAttribute("aria-pressed", "true");
  expect(await page.evaluate(() => document.documentElement.dataset.skin ?? null)).toBeNull();

  await page.emulateMedia({ colorScheme: "light" });
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.skin ?? null)).toBe("paper");

  await settings.locator('[data-skin-pick="warm"]').click();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await expect(settings.locator('[data-skin-pick="warm"]')).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.skin ?? null)).toBe("warm");
  await expect(settings).toContainText("Switches between Linear and Warm with the system appearance");

  await settings.locator('[data-skin-pick="tokyo"]').click();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await expect(settings.locator('[data-skin-pick="tokyo"]')).toHaveAttribute("aria-pressed", "true");
  expect(await page.evaluate(() => document.documentElement.dataset.skin ?? null)).toBe("warm");
  await expect(settings).toContainText("Switches between Tokyo Night and Warm with the system appearance");

  await page.emulateMedia({ colorScheme: "dark" });
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.skin ?? null)).toBe("tokyo");

  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  expect(await page.evaluate(() => document.documentElement.dataset.skin ?? null)).toBe("tokyo");
  await expect(settings.locator('[data-skin-pick="tokyo"]')).toHaveAttribute("aria-pressed", "true");
  await expect(settings.locator('[data-skin-pick="warm"]')).toHaveAttribute("aria-pressed", "false");
});

test("computer controls call the existing native commands", { tag: "@windows" }, async ({ page }, info) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "This computer", exact: true }).click();
  const toggle = settings.getByRole("switch", { name: "Let Beep use this computer" });
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  if (info.project.name === "macOS") {
    await settings.getByRole("button", { name: "Allow…" }).first().click();
    await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "allow_mac_permission" && call.args.permission === "ScreenRecording"))).toBe(true);
  } else {
    await expect(settings.getByRole("button", { name: "Allow…" })).toHaveCount(0);
  }
  await page.evaluate(() => window.__navigation.hold("set_mac_control"));
  await toggle.evaluate(el => Reflect.set(window, "computerSwitch", el));
  await toggle.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_mac_control" && call.args.enabled === false))).toBe(true);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(reads);
  await expect(toggle).toBeDisabled();
  expect(await toggle.evaluate(el => el === Reflect.get(window, "computerSwitch"))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  await expect(toggle).toBeDisabled();
  await page.evaluate(() => window.__navigation.release("set_mac_control"));
  await expect(toggle).toBeEnabled();
});
