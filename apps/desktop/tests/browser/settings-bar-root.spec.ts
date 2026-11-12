import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("bar Settings shares its page root and preserves controls on refresh", async ({ page }, info) => {
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    list_machines: [], list_credentials: [], list_github_accounts: [],
  });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar > *").first()).toBeVisible();
  await page.keyboard.press(`${info.project.name === "Windows" ? "Control" : "Meta"}+,`);
  const general = page.locator("#set-general");
  await expect(general).toBeVisible();
  const hint = page.locator("#bar .dhints");
  await hint.evaluate(node => Reflect.set(window, "settingsHints", node));
  const control = general.getByRole("switch").first();
  await control.focus();
  await control.evaluate(node => Reflect.set(window, "settingsControl", node));
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(reads);
  expect(await control.evaluate(node => node === Reflect.get(window, "settingsControl") && node === document.activeElement)).toBe(true);
  expect(await hint.evaluate(node => node === Reflect.get(window, "settingsHints"))).toBe(true);
  await expect(page.locator("#bar > [data-bar-page]")).toHaveCount(1);
});
