import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("chat refresh coalesces updates and preserves the latest reader position", async ({ page }) => {
  const user = { role: "user", text: "Question", at: 1, images: [], model: "", thinking: "" };
  const answer = (text: string) => ({ role: "assistant", html: `<p>${text} ${"Long answer ".repeat(200)}</p>`, tools: [], replies: [], model: "" });
  const initial = [user, answer("Initial")], older = [user, answer("Older snapshot")], latest = [user, answer("Latest snapshot")];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: initial, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const body = page.locator("#body");
  await expect(body).toContainText("Initial");
  await page.evaluate(({ older, latest }) => {
    const invoke = window.__TAURI__.core.invoke;
    let reads = 0;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "open_session") return invoke<T>(command, args);
      Reflect.set(window, "refreshReads", ++reads);
      if (reads === 1) return new Promise<T>(resolve => Reflect.set(window, "finishFirstRefresh", () => resolve(older as T)));
      return latest as T;
    };
  }, { older, latest });
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "refreshReads"))).toBe(1);
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  expect(await page.evaluate(() => Reflect.get(window, "refreshReads"))).toBe(1);
  await body.evaluate(el => { el.style.height = "100px"; el.style.flex = "0 0 100px"; el.style.overflow = "auto"; el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event("scroll")); el.scrollTop = 80; el.dispatchEvent(new Event("scroll")); });
  const top = await body.evaluate(el => el.scrollTop);
  expect(top).toBeGreaterThan(0);
  await page.evaluate(() => Reflect.get(window, "finishFirstRefresh")());
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "refreshReads"))).toBe(2);
  await expect(body).toContainText("Latest snapshot");
  await expect(body).not.toContainText("Older snapshot");
  await expect(body).toHaveJSProperty("scrollTop", top);
});

test("a history read started before a completed run cannot replace the new turn", async ({ page }) => {
  const user = { role: "user", text: "Original question", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Original answer</p>", tools: [], replies: [], model: "" };
  const nextUser = { ...user, text: "New question", at: 2 };
  const nextAnswer = { ...answer, html: "<p>New answer</p>" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await expect(page.locator("#body")).toContainText("Original answer");
  await page.evaluate(({ old, latest }) => {
    const invoke = window.__TAURI__.core.invoke;
    let reads = 0;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "open_session") {
        Reflect.set(window, "guardReads", ++reads);
        if (reads === 1) return new Promise<T>(resolve => Reflect.set(window, "finishOldRead", () => resolve(old as T)));
        return latest as T;
      }
      if (command === "send_prompt" && args?.onEvent && typeof args.onEvent === "object" && "onmessage" in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, "finishRun", () => { if (typeof channel.onmessage === "function") channel.onmessage({ kind: "finished", status: "completed" }); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  }, { old: [user, answer], latest: [user, answer, nextUser, nextAnswer] });
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "guardReads"))).toBe(1);
  await page.locator("#ask").fill("New question");
  await page.locator("#ask").press("Enter");
  await expect(page.locator("#body")).toContainText("New question");
  await page.evaluate(() => { Reflect.get(window, "finishRun")(); Reflect.get(window, "finishOldRead")(); });
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "guardReads"))).toBe(2);
  await expect(page.locator("#body")).toContainText("New answer");
  await expect(page.locator("#body")).toContainText("New question");
});
