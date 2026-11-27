import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;
const task = (id: string, name: string) => ({ id, name, description: name, state: "running", can_stop: true, output_file: null, summary: null, entry: null });
const three = [task("t1", "npm run dev"), task("t2", "cargo watch -x check"), task("t3", "tail -f node.log")];

/// Follows the session with `three` running; `window.setTasks(list)` sends a new list.
async function openSession(page: Page) {
  await openDesktop(page, true, { agent_sessions: [], agent_tasks: three, agent_steer: [] });
  await page.evaluate(tasks => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_follow" || args?.session !== "notice-one") return invoke<T>(command, args);
      const channel = args.onEvent as { onmessage(value: unknown): void };
      const base = await invoke<Record<string, unknown>>("agent_view", { session: "notice-one" });
      const entries = [{ kind: "user", text: "Start the dev servers" }];
      channel.onmessage({ kind: "snapshot", resumed: false, cursor: { start: 0, next: 1, fingerprint: "0" }, page: { ...base, entries, earlier: 0 }, modes: null, tasks });
      let at = 1;
      Reflect.set(window, "setTasks", (list: unknown[]) => {
        entries.push({ kind: "assistant", text: `Update ${at}` });
        channel.onmessage({ kind: "delta", resumed: false, cursor: { start: 0, next: at + 1, fingerprint: String(at) }, page: { ...base, entries: entries.slice(at), earlier: at }, modes: null, tasks: list });
        at += 1;
      });
      return new Promise<T>(() => {});
    };
  }, three);
  await page.evaluate(target => window.__navigation.open({ agent: target }), session);
}

const setTasks = (page: Page, list: unknown[]) => page.evaluate(value => (Reflect.get(window, "setTasks") as (list: unknown[]) => void)(value), list);

test("background commands fold into one line that opens the Running tab", async ({ page }) => {
  await openSession(page);
  const line = page.locator(".ag-tasks");
  await expect(line).toHaveText("3 commands running");
  await expect(page.locator("#agfield").getByRole("button", { name: "Stop", exact: true })).toBeHidden();
  expect((await line.boundingBox())!.height).toBeLessThanOrEqual(30);

  const tab = page.getByRole("tab", { name: /Running/ });
  await expect(tab).toContainText("3");
  await line.click();
  await expect(tab).toHaveAttribute("aria-selected", "true");
  const rows = page.locator(".agtaskrow");
  await expect(rows).toHaveCount(3);
  await expect(rows.first()).toContainText("npm run dev");

  await rows.filter({ hasText: "cargo watch" }).getByRole("button", { name: "Stop" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").map(c => c.args.steer))).toContainEqual({ stop_task: { task: "t2" } });

  await setTasks(page, [three[0], three[2], { ...three[1], state: "stopped" }]);
  await expect(line).toHaveText("2 commands running");
  await expect(rows).toHaveCount(2);
  await expect(tab).toContainText("2");

  await setTasks(page, []);
  await expect(line).toHaveCount(0);
  await expect(tab).toHaveCount(0);
});

test("on a phone the line opens the Running tab in place of the log", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openSession(page);
  await expect(page.locator("body")).toHaveClass(/mobile/);
  await page.locator(".ag-tasks").click();
  await expect(page.locator(".agtaskrow")).toHaveCount(3);
  await expect(page.locator(".agtaskrow").first()).toBeVisible();
  await expect(page.locator("#agbody")).toBeHidden();
  await page.screenshot({ path: "../../.artifacts/background-commands-phone.png" });

  const tabs = page.locator("#agmnav");
  const chat = tabs.getByRole("button", { name: "Chat", exact: true });
  const running = tabs.getByRole("button", { name: /Running/ });
  await expect(running).toHaveAttribute("aria-selected", "true");
  await chat.focus();
  await chat.press("Enter");
  await expect(chat).toBeFocused();
  await expect(chat).toHaveAttribute("aria-selected", "true");
  await expect(running).toHaveAttribute("aria-selected", "false");
  await expect(page.locator("#agbody")).toBeVisible();
});
