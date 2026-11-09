import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("chat navigation keeps prompt ticks while following reader scroll", async ({ page }) => {
  const history = Array.from({ length: 5 }, (_, index) => [
    { role: "user", text: `Question ${index}`, at: index + 1, images: [], model: "", thinking: "" },
    { role: "assistant", html: `<p>${"Answer ".repeat(50)}</p>`, tools: [], replies: [], model: "" },
  ]).flat();
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const body = page.locator("#body");
  await expect(body).toContainText("Question 4");
  await body.evaluate(el => { el.style.height = "100px"; el.style.flex = "0 0 100px"; el.style.overflow = "auto"; el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event("scroll")); el.scrollTop = 10; el.dispatchEvent(new Event("scroll")); });
  const map = page.locator("#map"), first = map.getByRole("button", { name: "Question 0" });
  await expect(first).toBeVisible();
  await expect(page.getByRole("button", { name: "Latest" })).toBeVisible();
  await first.focus();
  await first.evaluate(el => Reflect.set(window, "firstPromptTick", el));
  await body.evaluate(el => { el.scrollTop = 20; el.dispatchEvent(new Event("scroll")); });
  expect(await first.evaluate(el => el === Reflect.get(window, "firstPromptTick") && document.activeElement === el)).toBe(true);
  await page.getByRole("button", { name: "Latest" }).click();
  await expect(page.getByRole("button", { name: "Latest" })).toHaveCount(0);
  await expect(page.locator("#ask")).toBeFocused();
});
