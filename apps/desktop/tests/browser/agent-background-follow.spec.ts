import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const running = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;

/// Answers `agent_follow` with a stream the test pushes entries into with `window.pushEntry(text)`.
async function streamSession(page: Page) {
  await openDesktop(page, true, { agent_sessions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_follow" || args?.session !== "notice-one") return invoke<T>(command, args);
      const channel = args.onEvent as { onmessage(value: unknown): void };
      const base = await invoke<Record<string, unknown>>("agent_view", { session: "notice-one" });
      const entries = [{ kind: "user", text: "Check" }];
      channel.onmessage({ kind: "snapshot", resumed: false, cursor: { start: 0, next: 1, fingerprint: "0" }, page: { ...base, entries, earlier: 0 }, modes: null, tasks: [] });
      Reflect.set(window, "pushEntry", (text: string) => {
        const at = entries.length;
        entries.push({ kind: "assistant", text });
        channel.onmessage({ kind: "delta", resumed: false, cursor: { start: 0, next: at + 1, fingerprint: String(at) }, page: { ...base, entries: entries.slice(at), earlier: at }, modes: null, tasks: [] });
      });
      return new Promise<T>(() => {});
    };
  });
}

const count = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).length, command);
const push = (page: Page, text: string) => page.evaluate(value => (Reflect.get(window, "pushEntry") as (text: string) => void)(value), text);

test("a running session left for another view keeps streaming and shows it on return without a read", async ({ page }) => {
  await streamSession(page);
  await page.evaluate(session => window.__navigation.open({ agent: session }), running);
  await expect(page.locator("#agbody")).toContainText("Check");
  await push(page, "First reply");
  await expect(page.locator("#agbody")).toContainText("First reply");
  const follows = await count(page, "agent_follow");
  const reads = await count(page, "agent_view");

  await page.evaluate(() => window.__navigation.open("home"));
  await push(page, "Streamed while away");
  await page.evaluate(session => window.__navigation.open({ agent: session }), running);
  await expect(page.locator("#agbody")).toContainText("Streamed while away");
  expect(await count(page, "agent_follow")).toBe(follows);
  expect(await count(page, "agent_view")).toBe(reads);
  expect(await count(page, "agent_unfollow")).toBe(0);
});

test("a hidden window keeps the stream and draws it once when shown again", async ({ page }) => {
  await streamSession(page);
  await page.evaluate(session => window.__navigation.open({ agent: session }), running);
  await expect(page.locator("#agbody")).toContainText("Check");
  await push(page, "First reply");
  await expect(page.locator("#agbody")).toContainText("First reply");

  await page.evaluate(() => window.__navigation.emit("center-hidden", null));
  await push(page, "Streamed while hidden");
  await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
  await expect(page.locator("#agbody")).not.toContainText("Streamed while hidden");
  expect(await count(page, "agent_unfollow")).toBe(0);

  await page.evaluate(() => window.__navigation.emit("center-shown", null));
  await expect(page.locator("#agbody")).toContainText("Streamed while hidden");
  const shown = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_follow_show").map(call => call.args.id));
  expect(shown.at(-2)).toBeNull();
  expect(shown.at(-1)).toEqual(expect.any(String));
});
