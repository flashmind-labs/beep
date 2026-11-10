import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const running = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;
const task = { id: "t1", name: "npm run dev", description: "Background command", state: "running", can_stop: true, output_file: null, summary: null, entry: null };

const steers = (page: Page) => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer").map(call => call.args.steer));

/// Follows notice-one with one snapshot that holds `extra` (its tasks, its held steers).
async function follow(page: Page, extra: { tasks?: unknown[]; queued_messages?: string[] }) {
  await openDesktop(page, false, { agent_steer: [] });
  await page.evaluate(({ tasks = [], queued_messages = [] }) => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_follow" || args?.session !== "notice-one") return invoke<T>(command, args);
      const channel = args.onEvent as { onmessage(value: unknown): void };
      const base = await invoke<Record<string, unknown>>("agent_view", { session: "notice-one" });
      channel.onmessage({ kind: "snapshot", resumed: false, cursor: { start: 0, next: 1, fingerprint: "0" }, page: { ...base, entries: [{ kind: "user", text: "Check" }], earlier: 0, queued: queued_messages.length, queued_messages }, modes: null, tasks });
      return new Promise<T>(() => {});
    };
  }, extra);
  await page.evaluate(session => window.__navigation.open({ agent: session }), running);
  await expect(page.locator("#agbody")).toContainText("Check");
}

async function menuOnHome(page: Page) {
  await page.evaluate(() => window.__navigation.open("home"));
  await page.locator(".cwin").getByText("First notification").first().click({ button: "right" });
}

test("Stop all on a session with nothing else running stops at once", async ({ page }) => {
  await openDesktop(page);
  await menuOnHome(page);
  await page.getByRole("menuitem", { name: "Stop all" }).click();
  await expect.poll(() => steers(page)).toEqual(["stop_all"]);
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("Stop all asks first while background commands run, and the Stop button stops only the main turn", async ({ page }) => {
  await follow(page, { tasks: [task] });
  await expect(page.locator("#agstop")).toHaveAttribute("title", "Stop main turn (subagents keep running)");

  await menuOnHome(page);
  await page.getByRole("menuitem", { name: "Stop all" }).click();
  const sheet = page.getByRole("dialog");
  await expect(sheet).toContainText("While subagents run, the agent process is stopped. Your next message resumes it.");
  expect(await steers(page)).toEqual([]);
  await sheet.getByRole("button", { name: "Stop all" }).click();
  await expect.poll(() => steers(page)).toEqual(["stop_all"]);
});

test("Stop all puts the steers the node held back in the session's field", async ({ page }) => {
  await follow(page, { queued_messages: ["Use the fixture instead"] });
  await menuOnHome(page);
  await page.getByRole("menuitem", { name: "Stop all" }).click();
  await expect.poll(() => steers(page)).toEqual(["stop_all"]);
  await page.evaluate(session => window.__navigation.open({ agent: session }), running);
  await expect(page.locator("#agmsg")).toHaveValue("Use the fixture instead");
});
