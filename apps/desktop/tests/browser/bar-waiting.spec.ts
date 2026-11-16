import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const now = Date.now();
const session = (id: string, title: string, status: string, ago: number, question: unknown = null) => ({
  machine_id: "build-box", machine_name: "build-box", id, agent: id === "asks" ? "claude" : "codex", title, cwd: "/srv/project",
  status, doing: status === "working" ? "Running tests" : null, started_at: now - 3_600_000, last_activity: now - ago, external: false, question,
});
const question = { id: 1, text: "Run cargo test?", detail: "cargo test", options: [{ id: "allow", name: "Allow", allow: true }] };
const rows = [
  session("old", "Old idle work", "idle", 60_000),
  session("done", "Finished turn", "idle", 300_000),
  session("asks", "Asks a question", "asking", 600_000, question),
  session("runs", "Still running", "working", 30_000),
  session("gone", "Ended session", "ended", 10_000),
];
const target = (s: (typeof rows)[number]) => ({ agent: { ...s, kind: "agent" } });
const tree = { folders: [{ path: "", entries: [{ name: "src", folder: true, size: 0, ignored: false, change: null }, { name: "README.md", folder: false, size: 1, ignored: false, change: null }], more: 0, git: true }], truncated: false };
const notes = [{ id: "n1", kind: "finished", title: "Codex finished", text: "Done", source: "build-box", at: now, read: false, done: false, target: target(rows[1]) }];

async function openBar(page: Page, sessions: unknown[]) {
  await openDesktop(page, false, { agent_sessions: sessions, notes_list: notes, session_moves: [], agent_tree: tree });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar")).toBeAttached();
}
const shown = (page: Page) => page.evaluate(() => window.__navigation.emit("bar-shown", null));
const calls = (page: Page, names: string[]) => page.evaluate(n => window.__navigation.calls().filter(c => n.includes(c.command)).length, names);

test("double tap opens the waiting list; Enter opens a session to reply, Esc comes back", async ({ page }) => {
  await openBar(page, rows);
  await shown(page);
  const options = page.locator("#wtlist [role=option]");
  await expect(options).toHaveCount(4);
  await expect(options.nth(0)).toContainText("Asks a question");
  await expect(options.nth(0)).toContainText("Needs you");
  await expect(options.nth(1)).toContainText("Finished turn");
  await expect(options.nth(1)).toContainText("Done");
  await expect(options.nth(2)).toContainText("Still running");
  await expect(options.nth(3)).toContainText("Old idle work");
  await expect(page.locator("#wtlist")).toBeFocused();
  await expect(options.nth(0)).toHaveAttribute("aria-selected", "true");
  await page.screenshot({ path: "../../.artifacts/bar-waiting-list.png" });

  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  const composer = page.locator("#agmsg");
  await expect(composer).toBeFocused();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_follow" || c.command === "agent_view").map(c => c.args.session))).toContain("asks");
  await composer.fill("Go ahead");
  await composer.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").map(c => JSON.stringify(c.args)))).toContainEqual(expect.stringContaining("Go ahead"));
  await page.screenshot({ path: "../../.artifacts/bar-waiting-session.png" });

  await page.keyboard.press("Escape");
  await expect(page.locator("#wtlist")).toBeFocused();
  expect(await calls(page, ["hide_bar"])).toBe(0);
  await page.keyboard.press("Escape");
  await expect.poll(() => calls(page, ["hide_bar"])).toBe(1);
});

test("typing filters the list and Cmd+N opens Projects in the window", async ({ page }) => {
  await openBar(page, rows);
  await shown(page);
  await expect(page.locator("#wtlist [role=option]")).toHaveCount(4);
  await page.keyboard.type("finish");
  await expect(page.locator("#wtlist [role=option]")).toHaveCount(1);
  await expect(page.locator(".dhead small")).toHaveText("Filter: finish");
  await page.keyboard.press("Escape");
  await expect(page.locator("#wtlist [role=option]")).toHaveCount(4);
  await page.keyboard.press(test.info().project.name === "macOS" ? "Meta+n" : "Control+n");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "center_open").map(c => c.args.target))).toEqual([{ projects: { project: null } }]);
  await expect(page.locator("#agprompt")).toHaveCount(0);
});

test("with nothing recent the bar shows its empty list", async ({ page }) => {
  await openBar(page, [rows[4]]);
  await shown(page);
  await expect(page.locator(".dhead small")).toHaveText("Nothing waiting");
  await expect(page.locator("#agprompt")).toHaveCount(0);
  await page.screenshot({ path: "../../.artifacts/bar-waiting-empty.png" });
});

test("opening again with warm data reads nothing", async ({ page }) => {
  await openBar(page, rows);
  await shown(page);
  await expect(page.locator("#wtlist [role=option]")).toHaveCount(4);
  const reads = ["agent_sessions", "notes_list", "agent_places", "list_machines", "agent_hosts"];
  await page.waitForTimeout(300);
  const before = await calls(page, reads);
  await page.keyboard.press("Escape");
  await shown(page);
  await expect(page.locator("#wtlist [role=option]").first()).toContainText("Asks a question");
  await page.waitForTimeout(300);
  expect(await calls(page, reads)).toBe(before);
});

test("a start opened in the bar becomes the session its machine names, and Esc still comes back", async ({ page }) => {
  const start = { ...session("start:one", "Inspect the project", "starting", 1_000), doing: "Waiting for build-box" };
  const named = session("named", "Inspect the project", "working", 0);
  const pageOf = (row: typeof named, text: string | null) => ({ session: row, plan: [], files: [], entries: text === null ? [] : [{ kind: "user", text }], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0 });
  await openBar(page, [start]);
  await page.evaluate(first => window.__navigation.respond("agent_view", first), pageOf(start, "Inspect the project"));
  await shown(page);
  await expect(page.locator("#wtlist [role=option]")).toHaveCount(1);
  await page.keyboard.press("Enter");
  await expect(page.locator("#agconv .dbanner")).toContainText("Waiting for build-box");

  await page.evaluate(event => { window.__navigation.respond("agent_view", event.first); window.__navigation.emit("agent-started", event.started); },
    { first: pageOf(named, null), started: { start: start.id, session: named, first_message: "Inspect the project" } });
  await expect(page.locator("#agmsg")).toBeFocused();
  await expect(page.locator("#agbody .dcoderen.user")).toContainText("Inspect the project");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_follow").map(c => c.args.session))).toEqual(["named"]);

  await page.keyboard.press("Escape");
  await expect(page.locator("#wtlist")).toBeFocused();
});

test("a start that went from another window leaves its view", async ({ page }) => {
  const start = { ...session("start:one", "Inspect the project", "starting", 1_000), doing: "Starting Codex on build-box" };
  await openBar(page, [start]);
  await page.evaluate(first => window.__navigation.respond("agent_view", first),
    { session: start, plan: [], files: [], entries: [{ kind: "user", text: "Inspect the project" }], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0 });
  await shown(page);
  await page.keyboard.press("Enter");
  await expect(page.locator("#agconv .dbanner")).toContainText("Starting Codex on build-box");

  // This device no longer keeps it.
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => command === "agent_view" ? Promise.reject({ kind: "gone" }) : invoke<T>(command, args);
    window.__navigation.emit("sessions-changed", "start:one");
  });
  await expect(page.locator("#wtlist")).toBeFocused();
});

test("a session keeps its first message when it is left and opened again before its machine logs it", async ({ page }) => {
  const named = session("named", "Inspect the project", "starting", 0);
  const empty = (doing: string | null) => ({ session: { ...named, doing }, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0 });
  const update = (kind: string, doing: string | null) => ({ kind, resumed: false, cursor: { start: 0, next: 0, fingerprint: "empty" }, page: empty(doing), modes: null, tasks: [] });
  await openBar(page, [named]);
  // Its stream answers with the empty log, and stays open for what its machine says next.
  await page.evaluate(first => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const channel = args?.onEvent;
      if (command !== "agent_follow" || typeof channel !== "object" || channel === null || !("onmessage" in channel) || typeof channel.onmessage !== "function") return invoke<T>(command, args);
      const push = channel.onmessage.bind(channel);
      Reflect.set(window, "push", push);
      push(first);
      return new Promise<never>(() => {});
    };
  }, update("snapshot", null));
  await page.evaluate(started => window.__navigation.emit("agent-started", started), { start: "start:one", session: named, first_message: "Inspect the project" });
  await shown(page);
  await page.keyboard.press("Enter");
  const sent = page.locator("#agbody .dcoderen.user");
  await expect(sent).toContainText("Inspect the project");

  await page.keyboard.press("Escape");
  await expect(page.locator("#wtlist")).toBeFocused();
  await page.evaluate(next => Reflect.get(window, "push")(next), update("delta", "Setting the model"));
  await page.keyboard.press("Enter");
  await expect(page.locator("#agconv")).toBeVisible();
  await expect(sent).toContainText("Inspect the project");
});
