import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("diagram dialog keeps its sandboxed frame and restores focus", async ({ page }) => {
  const source = "graph TD; A-->B";
  const history = [{ role: "user", text: "Draw", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: `<pre><code class="language-mermaid">${source}</code></pre>`, tools: [], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const worker = page.locator('iframe[title="Diagram renderer"]');
  await expect(worker).toHaveCount(1);
  await page.evaluate(value => {
    const frame = document.querySelector<HTMLIFrameElement>('iframe[title="Diagram renderer"]');
    window.dispatchEvent(new MessageEvent("message", { source: frame?.contentWindow, data: { kind: "beep-diagram", action: "rendered", source: value, svg: "<svg></svg>" } }));
  }, source);
  const preview = page.locator('iframe[title="Mermaid diagram"]').first();
  await expect(preview).toHaveCount(1);
  const ask = page.locator("#ask"); await ask.focus();
  await preview.evaluate(frame => window.dispatchEvent(new MessageEvent("message", { source: (frame as HTMLIFrameElement).contentWindow, data: { kind: "beep-diagram", action: "expand" } })));
  const dialog = page.getByRole("dialog", { name: "Diagram" });
  await expect(dialog).toBeVisible();
  const frame = dialog.locator("iframe");
  await expect(frame).toHaveAttribute("sandbox", "allow-scripts");
  await expect(frame).toHaveAttribute("referrerpolicy", "no-referrer");
  await frame.evaluate(element => window.dispatchEvent(new MessageEvent("message", { source: (element as HTMLIFrameElement).contentWindow, data: { kind: "beep-diagram", action: "close" } })));
  await expect(dialog).toHaveCount(0);
  await expect(ask).toBeFocused();
});

test("a Mermaid block renders with Reduce motion on", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  const source = "graph TD; A-->B";
  const history = [{ role: "user", text: "Draw", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: `<pre><code class="language-mermaid">${source}</code></pre>`, tools: [], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await expect(page.locator('iframe[title="Diagram renderer"]')).toHaveCount(1);
  await page.evaluate(value => {
    const frame = document.querySelector<HTMLIFrameElement>('iframe[title="Diagram renderer"]');
    window.dispatchEvent(new MessageEvent("message", { source: frame?.contentWindow, data: { kind: "beep-diagram", action: "rendered", source: value, svg: "<svg></svg>" } }));
  }, source);
  await expect(page.locator('iframe[title="Mermaid diagram"]')).toHaveCount(1);
});
