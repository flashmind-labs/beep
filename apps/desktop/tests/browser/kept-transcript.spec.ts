import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const running = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;
const cursor = { start: 0, next: 2, fingerprint: "kept" };

const calls = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => call.args), command);

test("a session kept on disk draws at once and follows on from its cursor", async ({ page }) => {
  await openDesktop(page, false, { agent_sessions: [] });
  await page.evaluate(kept => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "transcript_kept") return invoke<T>(command, args);
      const base = await invoke<Record<string, unknown>>("agent_view", { session: "notice-one" });
      const usage = { context: null, tokens: 0, cost_usd: 0.25 };
      return { page: { ...base, usage, entries: [{ kind: "user", text: "Kept question" }, { kind: "assistant", text: "Kept answer" }], earlier: 0 }, cursor: kept } as T;
    };
    window.__navigation.hold("agent_follow");
  }, cursor);

  await page.evaluate(session => window.__navigation.open({ agent: session }), running);
  await expect(page.locator("#agbody")).toContainText("Kept answer");
  await expect(page.locator("#aguse")).toHaveText("$0.25");
  await expect.poll(() => calls(page, "agent_follow")).toHaveLength(1);
  expect((await calls(page, "agent_follow"))[0].after).toEqual(cursor);
  // The follow has not answered: no read goes around it.
  await page.waitForTimeout(300);
  expect((await calls(page, "agent_view")).filter(args => "last" in args)).toHaveLength(0);
});

test("a streaming session is kept by the native follow, with no keep call from the page", async ({ page }) => {
  await page.clock.install();
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

test("a kept log reopens at the latest message with its older entries still loaded", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    const entry = (i: number) => i % 2 ? { kind: "assistant", text: `Answer ${i}` } : { kind: "user", text: `Question ${i}` };
    const run = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => entry(from + i));
    window.__TAURI__.core.invoke = async <T>(command: string, args: Record<string, unknown> = {}) => {
      if (command === "agent_older") {
        await invoke(command, args);
        const before = Number(args.before), from = Math.max(0, before - 200);
        return { entries: run(from, before), earlier: from } as T;
      }

      if (command !== "agent_follow") return invoke<T>(command, args);
      const channel = args.onEvent as { onmessage(value: unknown): void };
      const base = await invoke<Record<string, unknown>>("agent_view", { session: args.session });
      const page = args.session === "notice-one" ? { ...base, entries: run(200, 400), earlier: 200 } : { ...base, entries: [entry(0), entry(1)], earlier: 0 };
      channel.onmessage({ kind: "snapshot", resumed: false, cursor: { start: page.earlier, next: page.earlier + page.entries.length, fingerprint: String(args.session) }, page, modes: null, tasks: [] });
      return new Promise<T>(() => {});
    };
  });
  const open = (id: string) => page.evaluate(session => window.__navigation.open({ agent: session }), { ...running, id });
  const body = page.locator("#agbody");

  await open("notice-one");
  await expect(body).toContainText("Answer 399");
  expect(await calls(page, "agent_older")).toHaveLength(0);
  await page.keyboard.press("Home");
  await expect(body).toContainText("Question 0");
  await body.evaluate(b => { b.scrollTop = 160; });
  await expect.poll(() => body.evaluate(b => b.scrollTop)).toBe(160);
  await expect(page.getByRole("button", { name: "Scroll to bottom" })).toBeVisible();

  await open("notice-two");
  await expect(body).toContainText("Answer 1");
  await open("notice-one");
  await expect(body.getByText("Answer 399", { exact: true })).toBeInViewport();
  await expect.poll(() => body.evaluate(b => b.scrollHeight - b.clientHeight - b.scrollTop)).toBeLessThan(2);
  await page.keyboard.press("Home");
  await expect(body).toContainText("Question 0");
  expect(await calls(page, "agent_older")).toHaveLength(1);
});

test("a deleted session's kept log is not drawn again", async ({ page }) => {
  const ended = { ...running, title: "First notification", status: "ended", question: null };
  await openDesktop(page, false, { agent_deletable: true, agent_delete: null, center_home: { machines: [], projects: [], running: [ended], needs_you: [] } });
  const body = page.locator("#agbody");
  await page.evaluate(session => window.__navigation.open({ agent: session }), ended);
  await expect(body).toHaveCount(1);
  await body.evaluate(b => { b.dataset.kept = "old"; });
  await page.evaluate(session => window.__navigation.open({ agent: session }), { ...ended, id: "notice-two" });
  await expect(body).not.toHaveAttribute("data-kept", "old");

  await page.evaluate(() => window.__navigation.open("home"));
  await page.locator(".cwin").getByText("First notification").first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
  await expect.poll(() => calls(page, "agent_delete")).toHaveLength(1);

  await page.evaluate(session => window.__navigation.open({ agent: session }), ended);
  await expect(body).toHaveCount(1);
  await expect(body).not.toHaveAttribute("data-kept", "old");
});

test("a kept log's clocks show the time at once when it comes back", async ({ page }) => {
  await page.clock.install();
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args: Record<string, unknown> = {}) => {
      if (command !== "agent_follow") return invoke<T>(command, args);
      const channel = args.onEvent as { onmessage(value: unknown): void };
      const base = await invoke<Record<string, unknown>>("agent_view", { session: args.session });
      const entries = [{ kind: "user", text: `Check ${String(args.session)}`, at: Date.now() }];
      channel.onmessage({ kind: "snapshot", resumed: false, cursor: { start: 0, next: 1, fingerprint: "0" }, page: { ...base, session: { ...base.session as object, status: args.session === "notice-two" ? "ended" : "working" }, entries, earlier: 0 }, modes: null, tasks: [] });
      return new Promise<T>(() => {});
    };
  });
  const open = (id: string, status = "working") => page.evaluate(session => window.__navigation.open({ agent: session }), { ...running, id, status });
  const clock = page.locator("#agbody [data-since]").first();

  await open("notice-one");
  await expect(page.locator("#agbody")).toContainText("Check notice-one");
  await expect(clock).toBeVisible();
  const before = await clock.textContent();
  // A session with no clock running: the timer stops.
  await open("notice-two", "ended");
  await expect(page.locator("#agbody")).toContainText("Check notice-two");
  await expect(page.locator("#agbody [data-since]")).toHaveCount(0);
  // Time stands still from here: only the reattach can move the clock.
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 125_000));

  await open("notice-one");
  await expect(page.locator("#agbody")).toContainText("Check notice-one");
  await expect(clock).not.toHaveText(before ?? "");
});
