import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("chat updates keep the composer, draft, caret and scroll", async ({ page }) => {
  const history = [{ role: "user", text: "Explain", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Answer</p>", tools: [], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const ask = page.locator("#ask"), body = page.locator("#body");
  await expect(page.locator(".dcoderen.agent")).toContainText("Answer");
  await ask.fill("draft text");
  await ask.evaluate(el => { if (el instanceof HTMLTextAreaElement) { el.setSelectionRange(2, 5); Reflect.set(window, "chatComposer", el); } });
  await body.evaluate(el => { el.style.height = "40px"; el.style.flex = "0 0 40px"; el.style.overflow = "auto"; el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event("scroll")); el.scrollTop = 10; el.dispatchEvent(new Event("scroll")); });
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  const top = await body.evaluate(el => el.scrollTop);
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await ask.evaluate(el => el === Reflect.get(window, "chatComposer") && document.activeElement === el && el instanceof HTMLTextAreaElement && el.value === "draft text" && el.selectionStart === 2 && el.selectionEnd === 5)).toBe(true);
  await expect(body).toHaveJSProperty("scrollTop", top);
  await expect(page.getByRole("separator", { name: "Resize the prompt" })).toBeVisible();
});
