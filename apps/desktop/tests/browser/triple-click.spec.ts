import { test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("triple-click selects only that prompt", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      return { ...value, entries: [
        { kind: "agent", text: "Working on it.", html: "<p>Working on it.</p>", sent_at: 1794579240 },
        { kind: "tool", text: "Run: cargo test [completed]", sent_at: 1794579241 },
        { kind: "user", text: "does it have threads?", html: "<p>does it have threads?</p>", sent_at: 1794579242 },
        { kind: "thought", text: "Thinking", ms: 2000, sent_at: 1794579243 },
        { kind: "tool", text: "Run: cargo build [completed]", sent_at: 1794579244 },
        { kind: "agent", text: "Not threads.", html: "<p>Not threads.</p><ul><li>Sessions</li></ul>", sent_at: 1794579245 },
        { kind: "user", text: "can a branch be summarized?", sent_at: 1794579246 },
        { kind: "agent", text: "Five of nine fixed.", html: "<p>Five of nine fixed.</p>", sent_at: 1794579247 },
      ] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  for (const text of ["does it have threads?", "can a branch be summarized?"]) {
    const prompt = page.locator(".dcoderen.user").filter({ hasText: text });
    const box = await prompt.boundingBox();
    for (const x of [30, (box?.width ?? 100) - 20]) {
      await prompt.click({ clickCount: 3, position: { x, y: 14 } });
      console.log(text, x, JSON.stringify(await page.evaluate(() => getSelection()?.toString())));
    }
  }
  console.log(await page.locator(".dcoderlog").evaluate(el => el.outerHTML.slice(0, 4000)));
});
