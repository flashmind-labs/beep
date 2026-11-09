import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("chat hint updates retain the input and running Stop control", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => { const invoke = window.__TAURI__.core.invoke; window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => command === "send_prompt" ? new Promise<T>(() => {}) : invoke<T>(command, args); });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const input = page.locator("#ask"), hints = page.locator("#bar .dhints");
  await input.fill("First request");
  await expect(hints).toContainText("ask");
  await input.press("Enter");
  const stop = page.locator("#stopRun");
  await expect(stop).toBeVisible();
  await stop.evaluate(el => Reflect.set(window, "chatStop", el));
  await input.fill("Queued text");
  await input.press("Enter");
  await expect(hints).toContainText("edit queued");
  expect(await stop.evaluate(el => el === Reflect.get(window, "chatStop"))).toBe(true);
  await stop.click();
  await expect(stop).toHaveCount(0);
  await expect(input).toBeFocused();
});
