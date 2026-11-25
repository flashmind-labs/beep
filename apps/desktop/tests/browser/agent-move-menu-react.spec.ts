import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const machines = [{ id: "build-box", name: "Build box", online: true, folder: "/srv/project", checking: false, checked_at: 1 }, { id: "other", name: "Other box", online: true, folder: "/srv/project", checking: false, checked_at: 1 }];
const hosts = [{ machine_id: "other", machine_name: "Other box", agents: ["codex"], workspace: "/srv/project", home: "/srv", capabilities: [] }];
async function openAgent(page: import("@playwright/test").Page) {
  await openDesktop(page, true, { project_machines: machines, agent_hosts: hosts, session_move_probe: true });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "Build box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await expect(page.locator("#agmoveb")).toBeVisible();
}

test("move menu keeps its button through target discovery and sends the chosen machine", async ({ page }) => {
  await openAgent(page);
  await page.evaluate(() => window.__navigation.hold("project_machines"));
  const button = page.locator("#agmoveb");
  await button.focus();
  await button.evaluate(el => Reflect.set(window, "moveButton", el));
  await button.click();
  await expect(page.getByText("Checking machines…")).toBeVisible();
  await page.evaluate(() => window.__navigation.release("project_machines"));
  const target = page.getByRole("menuitem", { name: /Other box/ });
  await expect(target).toBeEnabled();
  expect(await button.evaluate(el => el === Reflect.get(window, "moveButton") && document.activeElement === el)).toBe(true);
  await target.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "session_move").map(call => call.args.to))).toEqual(["other"]);
});

test("a closed move menu ignores its late discovery", async ({ page }) => {
  await openAgent(page);
  await page.evaluate(() => window.__navigation.hold("project_machines"));
  const button = page.locator("#agmoveb");
  await button.click();
  await expect(page.getByText("Checking machines…")).toBeVisible();
  await button.click();
  await page.evaluate(() => window.__navigation.release("project_machines"));
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator(".mvm")).toHaveCount(0);
});

test("a move pushed with the home lists shows at once and reads no sessions", async ({ page }) => {
  await openAgent(page);
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_sessions").length);
  const before = await reads();
  const session = { kind: "agent", machine_id: "build-box", machine_name: "Build box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false };
  const record = { session: "notice-one", agent: "codex", from: "build-box", to: "other", project: "", at: Date.now() };
  const progress = { answer: "known", phase: "conversation", copied: 0, total: 0, source_away: false, reason: null, worktree: null, worktree_at: 0, worktrees: 0, edits_left: false, files_copied: 0, files_total: 0 };
  await page.evaluate(views => window.__navigation.notify("moves", views), [{ command: "session_move_progress", args: { session: "notice-one" }, value: progress }]);
  await page.evaluate(views => window.__navigation.notify("home", views), [
    { command: "agent_sessions", args: {}, value: [session] },
    { command: "session_moves", args: {}, value: [record] },
  ]);
  await expect(page.locator("#agmovebox")).toContainText("Moving to");
  expect(await reads()).toBe(before);
});

test("Esc closes the move menu first, and a session left and opened again shows it closed", async ({ page }) => {
  await openAgent(page);
  const button = page.locator("#agmoveb"), session = { kind: "agent", machine_id: "build-box", machine_name: "Build box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;
  await button.click();
  await expect(page.getByRole("menuitem", { name: /Other box/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(button).toBeVisible();
  await expect(button).toHaveAttribute("aria-expanded", "false");

  await button.click();
  await expect(page.getByRole("menuitem", { name: /Other box/ })).toBeVisible();
  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(target => window.__navigation.open({ agent: target }), session);
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator(".mvm")).toHaveCount(0);
  await button.click();
  await expect(page.getByRole("menuitem", { name: /Other box/ })).toBeVisible();
});

test("an assistant chat moves with its threads to a machine that has their harnesses", async ({ page }) => {
  const folder = "/srv/.beep/assistant/chat-one";
  const chat = { machine_id: "build-box", machine_name: "Build box", id: "chat-one", agent: "flsh", title: "Plan the trip", cwd: folder, status: "idle", doing: null, started_at: 1, last_activity: 1, external: false, question: null, assistant: true, model: null };
  const thread = { ...chat, id: "thread-one", agent: "claude", title: "Research", cwd: `${folder}/research`, assistant: false, started_at: 2, status: "ended", parent: { machine_id: "build-box", id: "chat-one", key: "" } };
  const host = (machine_id: string, machine_name: string, agents: string[]) => ({ machine_id, machine_name, agents, workspace: null, home: "/srv", capabilities: [], assistant: true, moves_chats: true });
  await openDesktop(page, false, {
    agent_hosts: [host("build-box", "Build box", ["flsh", "claude"]), host("other", "Other box", ["flsh", "claude"]), host("small", "Small box", ["flsh"])],
    agent_sessions: [chat, thread], chat_move: [],
    agent_view: { session: chat, plan: [], entries: [], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1 },
  });
  await page.evaluate(target => window.__navigation.open({ agent: { kind: "agent", ...target } }), chat);

  await page.locator("#agmoveb").click();
  await expect(page.getByRole("menuitem", { name: /Small box/ })).toBeDisabled();
  await expect(page.getByRole("menuitem", { name: /Small box/ })).toContainText("no Claude Code here");
  await page.getByRole("menuitem", { name: /Other box/ }).click();

  const calls = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "chat_move").map(call => call.args));
  await expect.poll(calls).toEqual([{
    machineId: "build-box", to: "other", stop: false,
    sessions: [
      { agent: "flsh", session: "chat-one", cwd: folder, model: null, ended: false },
      { agent: "claude", session: "thread-one", cwd: `${folder}/research`, model: null, ended: true },
    ],
  }]);
});
