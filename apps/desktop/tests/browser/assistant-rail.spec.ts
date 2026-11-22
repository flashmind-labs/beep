import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const host = { machine_id: "build-box", machine_name: "build-box", agents: ["claude", "flsh"], workspace: null, home: "/srv", capabilities: [], assistant: true, moves_chats: true };
const folder = "/srv/.beep/assistant/chat-one";
const chat = { machine_id: "build-box", machine_name: "build-box", id: "chat-one", agent: "flsh", title: "Plan the launch", cwd: folder, status: "idle", doing: null as string | null, started_at: 1, last_activity: 5, external: false, question: null, assistant: true, parent: null as { machine_id: string; id: string; key: string } | null };
const under = { machine_id: "build-box", id: "chat-one", key: "" };
const research = { ...chat, id: "thread-one", agent: "claude", title: "Pricing research", cwd: `${folder}/pricing`, status: "working", doing: "Reading the price list", started_at: 2, assistant: false, parent: under };
const notes = { ...chat, id: "thread-two", agent: "claude", title: "Competitor notes", cwd: `${folder}/notes`, status: "ended", started_at: 3, last_activity: 4, assistant: false, parent: under };
const view = (row: typeof chat) => ({ session: row, plan: [], files: [], entries: [{ kind: "agent", text: "On it." }], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 1 });

async function openChat(page: Page, sessions: (typeof chat)[]) {
  await openDesktop(page, false, { agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: sessions, agent_view: view(chat) });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click();
}

const mod = (info: TestInfo) => info.project.name === "Windows" ? "Control" : "Meta";

test("a chat with threads lists them in place of the sidebar, and ⌘E swaps them", async ({ page }, info) => {
  await openChat(page, [chat, research, notes]);
  const rail = page.locator("#rail");

  await expect(page.locator(".cwin")).toHaveClass(/rail-on/);
  await expect(rail.locator(".rail-live .pj-li")).toHaveText([/Plan the launch/, /Pricing research.*Reading the price list/]);
  // The running threads are the chat's subitems, indented under it.
  await expect(rail.locator(".rail-threads .pj-li")).toHaveText([/Pricing research/]);
  expect(await rail.locator(".rail-threads .pj-li").evaluate(el => el.getBoundingClientRect().left)).toBeGreaterThan(await rail.locator(".rail-live > .pj-li").first().evaluate(el => el.getBoundingClientRect().left));
  await expect(rail.locator(".rail-scroll > .pj-li")).toHaveText([/Competitor notes/]);
  await expect(rail.locator('[aria-current="true"]')).toContainText("Plan the launch");
  await expect(page.locator(".asst-list")).toBeHidden();

  await page.evaluate(() => window.__navigation.respond("agent_view", null));
  await rail.getByRole("button", { name: /Pricing research/ }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_view").at(-1)?.args.session)).toBe("thread-one");
  await expect(rail).toBeVisible();

  await page.keyboard.press(`${mod(info)}+e`);
  await expect(page.locator(".cwin")).not.toHaveClass(/rail-on/);
  await expect(page.locator("#side")).toBeVisible();
  await expect(page.locator(".asst-list")).toBeVisible();
});

test("a chat without threads keeps the chats beside it", async ({ page }) => {
  await openChat(page, [chat]);
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#agmsg")).toBeVisible();
  await expect(page.locator(".cwin")).not.toHaveClass(/rail-on/);
});

test("⌘N beside a chat's threads asks it for a new thread, and ⇧⌘N opens the chat in a window", async ({ page }, info) => {
  await openChat(page, [chat, research]);
  await expect(page.locator("#agmsg")).toBeVisible();

  await page.keyboard.press(`${mod(info)}+n`);
  await expect(page.locator("#agmsg")).toHaveValue(/^New thread: /);

  await page.keyboard.press(`${mod(info)}+Shift+n`);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_open_window").at(-1)?.args.alone)).toEqual({ kind: "chat", machine_id: "build-box", session: "chat-one" });
});

test("⌘N beside the chats starts a new chat", async ({ page }, info) => {
  await openChat(page, [chat]);
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#agmsg")).toBeVisible();

  await page.keyboard.press(`${mod(info)}+n`);
  await expect(page.locator("#agprompt")).toHaveAttribute("placeholder", "Ask the assistant.");
});

test("a chat's pane has its files without its threads' folders, and no Changes or threads tab", async ({ page }) => {
  const entry = (name: string, folder: boolean) => ({ name, folder, size: 1, ignored: false, change: null });
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat, research, notes], agent_view: view(chat),
    agent_tree: { folders: [{ path: "", entries: [entry("pricing", true), entry("notes", true), entry("drafts", true), entry("notes.md", false)], more: 0, git: false }], truncated: false },
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click();

  const strip = page.locator("#agpstrip-right, #agpane-right");
  await page.getByRole("tab", { name: "Files", exact: true }).first().click();
  await expect(page.locator('[data-fpath="notes.md"]')).toBeVisible();
  await expect(page.locator('[data-fpath="drafts"]')).toBeVisible();
  await expect(page.locator('[data-fpath="pricing"]')).toHaveCount(0);
  await expect(page.locator('[data-fpath="notes"]')).toHaveCount(0);
  await expect(strip.getByRole("tab", { name: "Changes", exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree").at(-1)?.args.root)).toBe(folder);
});

test("a chat's move counts its files as they come, with the bar over the input, without waiting for the mesh", async ({ page }) => {
  const record = { session: "chat-one", agent: "flsh", from: "build-box", to: "other", project: "", at: Date.now() };
  const progress = (over: object) => ({ answer: "known", phase: "waiting", copied: 0, total: 0, source_away: false, reason: null, worktree: null, worktree_at: 0, worktrees: 0, edits_left: false, files_copied: 0, files_total: 0, ...over });
  await openDesktop(page, false, {
    agent_hosts: [host, { ...host, machine_id: "other", machine_name: "Other box" }], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null },
    agent_sessions: [chat], agent_view: view(chat), session_moves: [record], session_move_progress: progress({}),
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click();
  await expect(page.locator("#agmovebox")).toContainText("Waiting for Other box");

  // Only the target knows how far it is: the window asks it again while the move shows.
  await page.evaluate(next => window.__navigation.respond("session_move_progress", next), progress({ phase: "files", total: 1_000_000, files_copied: 120_000_000, files_total: 551_000_000 }));
  await expect(page.locator("#agmovebox")).toContainText("Files 120.0 MB of 551.0 MB · 22%");
  await expect(page.locator("#agfield").getByRole("progressbar", { name: "Moving to Other box" })).toHaveAttribute("aria-valuenow", "22");
});

test("a file opened from a chat's Files sits beside the chat, and Markdown opens in its preview", async ({ page }) => {
  const entry = (name: string, folder: boolean) => ({ name, folder, size: 1, ignored: false, change: null });
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat], agent_view: view(chat),
    agent_tree: { folders: [{ path: "", entries: [entry("notes.md", false)], more: 0, git: false }], truncated: false },
    agent_artifact: { kind: "html", html: "<h1>Notes</h1>" },
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click();
  await page.getByRole("tab", { name: "Files", exact: true }).first().click();
  await page.locator('[data-fpath="notes.md"]').click();

  await expect(page.locator("#agpane-right").getByRole("button", { name: "Source", exact: true })).toBeVisible();
  await expect(page.locator(".agshell")).not.toHaveClass(/zoomed/);
  await expect(page.locator(".asst-list")).toBeHidden();
  await expect(page.getByText("On it.")).toBeVisible();
});

test("a chat's pane lists the agents its folder defines in an Agents tab", async ({ page }) => {
  const agent = (name: string, scope: "project" | "user") => ({ name, harness: "claude", scope, description: `${name} does things`, model: null, tools: [], color: null, path: `.claude/agents/${name}.md`, prompt: "You check evidence.", prompt_cut: false, error: null });
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat], agent_view: view(chat),
    custom_agents: [agent("evidence-verifier", "project"), agent("mine-everywhere", "user")],
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click();

  const tab = page.getByRole("tab", { name: /Agents/ }).first();
  await expect(tab).toContainText("1");
  await tab.click();
  const pane = page.locator(".agagents");
  await expect(pane.getByText("evidence-verifier", { exact: true })).toBeVisible();
  await expect(pane.getByText("mine-everywhere")).toHaveCount(0);
  await expect(pane.getByRole("button", { name: "Start" })).toHaveCount(0);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "custom_agents").at(-1)?.args.folder)).toBe(folder);
});

test("a chat's window shows the chat beside its threads, and ⌘N there asks for a new thread", async ({ page }, info) => {
  await openDesktop(page, false, {
    center_window_alone: { kind: "chat", machine_id: "build-box", session: "chat-one" },
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat], agent_view: view(chat),
  });
  const rail = page.locator("#rail");

  await expect(page.locator("body")).toHaveClass(/\bpw\b/);
  await expect(page.locator("#agmsg")).toBeVisible();
  // No threads yet, and still the rail: a chat's window has no sidebar.
  await expect(rail.locator('[aria-current="true"]')).toContainText("Plan the launch");
  await expect(page.locator("#side")).toBeHidden();
  await expect(page.locator(".asst-list")).toBeHidden();

  await page.keyboard.press(`${mod(info)}+n`);
  await expect(page.locator("#agmsg")).toHaveValue(/^New thread: /);
});

test("a chat shows what its threads do, and the Assistant link counts the chats waiting on an answer", async ({ page }) => {
  const asks = { ...research, id: "thread-asks", title: "Budget check", status: "asking", last_activity: 50 };
  const other = { ...chat, id: "chat-two", title: "Write the post", last_activity: 9 };
  await openDesktop(page, false, { agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat, other, research, asks], agent_view: view(chat) });
  const link = page.locator("#side").getByRole("button", { name: /^Assistant/ });
  await expect(link).toContainText("1");
  await link.click();

  const list = page.locator(".asst-list");
  // The chat whose thread asked last comes first, though the other one spoke after it did.
  await expect(list.getByRole("button", { name: /Plan the launch|Write the post/ }).first()).toContainText("Plan the launch");
  const first = list.getByRole("button", { name: /Plan the launch/ });
  await expect(first).toContainText("thread needs you");
  await expect(first).toContainText("1 thread working");
});

test("a chat's pane folded once opens beside the chat again", async ({ page }) => {
  const entry = (name: string, folder: boolean) => ({ name, folder, size: 1, ignored: false, change: null });
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat], agent_view: view(chat),
    agent_tree: { folders: [{ path: "", entries: [entry("notes.txt", false), entry("plan.txt", false)], more: 0, git: false }], truncated: false },
  });
  await page.locator("#side").getByRole("button", { name: /^Assistant/ }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click();
  const pane = page.locator("#agpane-right");
  await page.getByRole("tab", { name: "Files", exact: true }).first().click();
  await page.locator('[data-fpath="notes.txt"]').click();
  await expect(pane).toBeVisible();

  await pane.locator('[data-pact="fold"]').click();
  await expect(pane).toBeHidden();
  await expect(page.locator(".asst-list")).toBeVisible();
  await page.locator("#agpstrip-right").getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="plan.txt"]').click();
  await page.waitForTimeout(300);
  await expect(pane).toBeVisible();
  await expect(page.locator(".asst-list")).toBeHidden();
  await expect(page.locator(".agshell")).not.toHaveClass(/zoomed/);
});
