import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const running = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;
const cursor = { start: 0, next: 2, fingerprint: "kept" };

const calls = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => call.args), command);

test("a session kept on disk draws at once and follows on from its cursor", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(kept => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "transcript_kept") return invoke<T>(command, args);
      const base = await invoke<Record<string, unknown>>("agent_view", { session: "notice-one" });
      return { page: { ...base, entries: [{ kind: "user", text: "Kept question" }, { kind: "assistant", text: "Kept answer" }], earlier: 0 }, cursor: kept } as T;
    };
    window.__navigation.hold("agent_follow");
  }, cursor);

  await page.evaluate(session => window.__navigation.open({ agent: session }), running);
  await expect(page.locator("#agbody")).toContainText("Kept answer");
  await expect.poll(() => calls(page, "agent_follow")).toHaveLength(1);
  expect((await calls(page, "agent_follow"))[0].after).toEqual(cursor);
  // The follow has not answered: no read goes around it.
  await page.waitForTimeout(300);
  expect((await calls(page, "agent_view")).filter(args => "last" in args)).toHaveLength(0);
});

test("a streaming session is kept by the native follow, with no keep call from the page", async ({ page }) => {
  await page.clock.install();
  await openDesktop(page, true);
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

  await page.evaluate(session => window.__navigation.open({ agent: session }), running);
  await expect(page.locator("#agbody")).toContainText("Check");
  const before = await page.evaluate(() => window.__navigation.calls().length);

  // A minute of streaming, one update a second.
  for (let second = 0; second < 60; second++) {
    await page.evaluate(value => (Reflect.get(window, "pushEntry") as (text: string) => void)(value), `Line ${second}`);
    await page.clock.runFor(1000);
  }

  await expect(page.locator("#agbody")).toContainText("Line 59");
  const minute = await page.evaluate(() => window.__navigation.calls().map(call => call.command));
  console.log(`IPC in a streaming minute: ${minute.length - before}, transcript_keep: ${minute.filter(command => command === "transcript_keep").length}`);
  await page.evaluate(() => window.__navigation.open("home"));
  await page.clock.runFor(1000);
  expect(await calls(page, "transcript_keep")).toHaveLength(0);
});
