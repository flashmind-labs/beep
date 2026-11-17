import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const responses = {
  settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
  mac_control: { id: "one", name: "build-box", enabled: true, status: "connected", error: null, permissions: { screen_recording: false, accessibility: false } },
};

test("desktop settings expose local hosting and native integrations", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Settings" });
  await sheet.getByRole("button", { name: "General", exact: true }).click();
  await expect(sheet.getByRole("switch", { name: "Launch at login" })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Choose…", exact: true })).toBeVisible();
  await sheet.getByRole("button", { name: "This computer", exact: true }).click();
  await expect(sheet.getByRole("switch", { name: "Let Beep use this computer" })).toBeVisible();
});

test("invalid platform capabilities keep native actions unavailable", async ({ page }) => {
  await openDesktop(page, false, { ...responses, build_capabilities: { local_host: true } });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Settings" });
  await sheet.getByRole("button", { name: "General", exact: true }).click();
  await expect(sheet.getByRole("switch", { name: "Launch at login" })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Choose…", exact: true })).toHaveCount(0);
  await sheet.getByRole("button", { name: "This computer", exact: true }).click();
  await expect(sheet.getByRole("switch", { name: "Let Beep use this computer" })).toHaveCount(0);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "build_capabilities").length)).toBe(1);
});
