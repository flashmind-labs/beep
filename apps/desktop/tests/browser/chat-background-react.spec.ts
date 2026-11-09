import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("background questions preserve existing controls and reader scroll", async ({ page }) => {
  const history = [{ role: "user", text: "Question", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: `<p>${"Long answer ".repeat(200)}</p>`, tools: [], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [], list_apps: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await expect(page.locator("#body")).toContainText("Long answer");
  const question = { session_id: "chat-example", query_id: "one", app: "Example", action: "Read data", fields: [] };
  await page.evaluate(value => window.__navigation.emit("work-question", value), question);
  const first = page.locator('[data-q="one"]'), allow = first.locator('[data-d="allow"]');
  await expect(allow).toBeVisible();
  await allow.focus();
  await allow.evaluate(el => Reflect.set(window, "backgroundAllow", el));
  const body = page.locator("#body");
  await body.evaluate(el => { el.style.height = "100px"; el.style.flex = "0 0 100px"; el.style.overflow = "auto"; el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event("scroll")); el.scrollTop = 10; el.dispatchEvent(new Event("scroll")); });
  const top = await body.evaluate(el => el.scrollTop);
  await page.evaluate(value => window.__navigation.emit("work-question", value), { ...question, query_id: "two", action: "Write data" });
  await expect(page.locator('[data-q="two"]')).toBeAttached();
  expect(await allow.evaluate(el => el === Reflect.get(window, "backgroundAllow") && document.activeElement === el)).toBe(true);
  await expect(body).toHaveJSProperty("scrollTop", top);
  await allow.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "answer_work").map(call => call.args.queryId))).toEqual(["one"]);
});
