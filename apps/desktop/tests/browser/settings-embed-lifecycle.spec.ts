import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

async function openSettingsWithInstructions(page: Page) {
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Instructions", exact: true }).click();
  await expect(settings.getByRole("textbox", { name: "Your instructions" })).toBeVisible();
  return settings;
}

const responses = {
  settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false },
  mac_control: { id: "build-box", name: "build-box", enabled: false, status: "off", error: null, permissions: { screen_recording: false, accessibility: false } },
  instructions_get: { version: 1, text: "Example instructions", note: "", saved_at: 0 },
};

test("closing embedded Instructions unmounts it when Home is underneath", async ({ page }) => {
  await openDesktop(page, false, responses);
  const settings = await openSettingsWithInstructions(page);
  await settings.locator("#set-instr-host").evaluate(host => { Reflect.set(window, "savedInstructionHost", host); });
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  expect(await page.evaluate(() => { const host: unknown = Reflect.get(window, "savedInstructionHost"); return host instanceof HTMLElement ? host.childElementCount : -1; })).toBe(0);
});

test("closing embedded Instructions remounts the underlying Instructions page", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.evaluate(() => window.__navigation.open("instructions"));
  await expect(page.getByRole("textbox", { name: "Your instructions" })).toBeVisible();
  const settings = await openSettingsWithInstructions(page);
  await settings.getByRole("textbox", { name: "Your instructions" }).fill("Draft instructions");
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "Your instructions" })).toHaveValue("Draft instructions");
});
