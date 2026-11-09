import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("a detached running chat is retained until its stream finishes", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: "chat-example" }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [], list_researches: [] });
  await page.goto("https://app.test/index.html");
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "send_prompt" && args?.onEvent && typeof args.onEvent === "object" && "onmessage" in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, "chatEvent", (event: unknown) => { if (typeof channel.onmessage === "function") channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.emit("bar-shown", "open"));
  await page.locator("#ask").fill("Running request");
  await page.locator("#ask").press("Enter");
  await expect(page.locator("#stopRun")).toBeVisible();
  await page.locator("#body").evaluate(el => Reflect.set(window, "oldChatBody", el));
  await page.evaluate(() => window.__navigation.emit("bar-shown", "research"));
  await expect(page.getByPlaceholder("What should the team research?")).toBeVisible();
  expect(await page.evaluate(() => Boolean(Reflect.get(window, "oldChatBody").querySelector(".turn")))).toBe(true);
  await page.evaluate(() => Reflect.get(window, "chatEvent")({ kind: "finished", status: "completed" }));
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "oldChatBody").querySelectorAll(".turn").length)).toBe(0);
  await page.evaluate(() => window.__navigation.emit("bar-shown", "open"));
  await expect(page.locator("#ask")).toBeVisible();
  expect(await page.locator("#body").evaluate(el => el !== Reflect.get(window, "oldChatBody"))).toBe(true);
});

test("following chat observes actual turn growth inside display contents hosts", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.goto("https://app.test/index.html");
  await page.evaluate(() => { const invoke = window.__TAURI__.core.invoke; window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => command === "send_prompt" ? new Promise<T>(() => {}) : invoke<T>(command, args); });
  await page.locator("#ask").fill("Observe this turn");
  await page.locator("#ask").press("Enter");
  const body = page.locator("#body"), turn = body.locator(".turn");
  await expect(turn).toBeVisible();
  await body.evaluate(el => { el.style.height = "40px"; el.style.flex = "0 0 40px"; el.style.overflow = "auto"; Reflect.set(el, "stuck", true); });
  await turn.evaluate(el => { (el as HTMLElement).style.height = "160px"; (el as HTMLElement).style.flex = "none"; });
  await expect.poll(() => body.evaluate(el => el.scrollTop)).toBeGreaterThan(0);
  await body.evaluate(el => { el.scrollTop = 0; Reflect.set(el, "stuck", false); });
  await turn.evaluate(el => { (el as HTMLElement).style.height = "240px"; });
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await expect(body).toHaveJSProperty("scrollTop", 0);
});
