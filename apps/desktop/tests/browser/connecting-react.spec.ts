import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("connecting retry keeps its button focused and opens Home when ready", async ({ page }) => {
  await page.clock.install();
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, center_home: null });
  await expect(page.getByText("Connecting to Beep…")).toBeVisible();
  await page.clock.fastForward(16000);
  await page.clock.resume();
  const retry = page.getByRole("button", { name: "Retry now" });
  await expect(retry).toBeVisible();
  await page.evaluate(() => window.__navigation.hold("center_home"));
  await retry.click();
  await expect(retry).toBeDisabled();
  await expect(retry).toBeFocused();
  await page.evaluate(() => { window.__navigation.respond("center_home", { machines: [], projects: [], running: [], needs_you: [], coming_up: null, updates: null }); window.__navigation.release("center_home"); });
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();
});

test("a failed retry becomes available with a cached Home", async ({ page }) => {
  await page.addInitScript(() => {
    const define = Object.defineProperty;
    Object.defineProperty = (target, key, descriptor) => {
      if (target === window && key === "__TAURI__") {
        const core = descriptor.value.core, invoke = core.invoke;
        core.invoke = (command: string, args: unknown) => command === "center_home" ? Promise.reject("not linked to a phone yet") : invoke(command, args);
        Object.defineProperty = define;
      }
      return define(target, key, descriptor);
    };
  });
  const home = { machines: [], projects: [], running: [], needs_you: [], coming_up: null, updates: null };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, center_cached: { home } });
  await expect(page.getByText("This computer is not linked yet")).toBeVisible();
  const retry = page.getByRole("button", { name: "Try again" });
  await expect(retry).toBeEnabled();
  await retry.click();
  await expect(retry).toBeEnabled();
});
