import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("thread card expands without replacing the original post", async ({ page }) => {
  const group = { project_id: "project", name: "example/widget", chat_id: "project-chat", open_tasks: 0, open_workers: 0 };
  const thread = { chat_id: "thread-chat", kind: "issue", repo: "example/widget", number: 1, title: "Issue" };
  const card = { kind: "issue", number: 1, title: "Issue", url: "https://example.test/issue", author: "Author", body: "Original post", needs_reply: false, earlier: 0, comments: Array.from({ length: 4 }, (_, i) => ({ author: `User ${i}`, at: 1000 + i, body: `Comment ${i}` })) };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, center_projects: [group], center_thread_chats: [thread], center_thread_view: card, open_session: [] });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_thread_chats").length)).toBeGreaterThan(0);
  await page.evaluate(() => window.__navigation.open({ chat: { session: "thread-chat" } }));
  const post = page.locator(".th-card .tx").first();
  await expect(post).toHaveText("Original post");
  await expect(page.locator("#body > .th-card.dchattop:first-child")).toHaveCount(1);
  await post.evaluate(element => { element.dataset.identity = "kept"; const range = document.createRange(); range.selectNodeContents(element); window.getSelection()?.removeAllRanges(); window.getSelection()?.addRange(range); });
  await page.getByRole("button", { name: "Show 1 earlier comment" }).dispatchEvent("click");
  await expect(post).toHaveAttribute("data-identity", "kept");
  await expect(page.locator(".th-card")).toContainText("Comment 0");
  expect(await page.evaluate(() => String(window.getSelection()))).toBe("Original post");
});
