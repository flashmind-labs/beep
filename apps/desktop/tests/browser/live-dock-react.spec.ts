import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("voice updates keep dock controls and microphone level", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: true, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.goto("https://app.test/index.html");
  const view = { kind: "view", phase: "listening", words: "First words", line: null, prev: null, step: null, speaking: false, pause_sends: false, session: null, title: null, hold_only: false, finishing: false, sheet: false };
  await page.evaluate(value => window.__navigation.emit("live", value), view);
  const orb = page.locator("#orb"), caption = page.locator("#ldock .cap");
  await expect(caption).toContainText("First words");
  await orb.focus();
  await orb.evaluate(el => Reflect.set(window, "savedOrb", el));
  await page.evaluate(() => window.__navigation.emit("live", { kind: "level", level: 0.5 }));
  await page.evaluate(value => window.__navigation.emit("live", value), { ...view, words: "More words" });
  await expect(caption).toContainText("More words");
  expect(await orb.evaluate(el => el === Reflect.get(window, "savedOrb") && document.activeElement === el && el.style.getPropertyValue("--lvl") === "0.500")).toBe(true);
  await page.evaluate(value => window.__navigation.emit("live", value), { ...view, words: "More words", sheet: true });
  const sheet = page.getByRole("dialog", { name: "What to say" });
  await expect(sheet).toContainText("Send now");
  await page.evaluate(value => window.__navigation.emit("live", value), { ...view, words: "More words", sheet: false });
  await expect(sheet).toHaveCount(0);
  await expect(orb).toBeFocused();
  await orb.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "live_tap"))).toBe(true);
  await page.locator("#lleave").click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "cancel_live"))).toBe(true);
});
