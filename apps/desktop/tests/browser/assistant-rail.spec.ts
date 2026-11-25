import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const host = { machine_id: "build-box", machine_name: "build-box", agents: ["claude", "flsh"], workspace: null, home: "/srv", capabilities: [], assistant: true, moves_chats: true };
const folder = "/srv/.beep/assistant/chat-one";
const chat = { machine_id: "build-box", machine_name: "build-box", id: "chat-one", agent: "flsh", title: "Plan the launch", cwd: folder, status: "idle", doing: null as string | null, started_at: 1, last_activity: 5, external: false, question: null, assistant: true, parent: null as { machine_id: string; id: string; key: string } | null };
const under = { machine_id: "build-box", id: "chat-one", key: "" };
const research = { ...chat, id: "thread-one", agent: "claude", title: "Pricing research", cwd: `${folder}/pricing`, status: "working", doing: "Reading the price list", started_at: 2, assistant: false, parent: under };
const notes = { ...chat, id: "thread-two", agent: "claude", title: "Competitor notes", cwd: `${folder}/notes`, status: "ended", started_at: 3, last_activity: 4, assistant: false, parent: under };
const view = (row: typeof chat) => ({ session: row, plan: [], entries: [{ kind: "agent", text: "On it." }], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1 });
const entry = (name: string, folder = false) => ({ name, folder, size: 1, ignored: false, change: null });
const tree = (...entries: ReturnType<typeof entry>[]) => ({ folders: [{ path: "", entries, more: 0 }], truncated: false });

async function openChat(page: Page, sessions: (typeof chat)[], listing = tree()) {
  await openDesktop(page, false, { agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: sessions, agent_view: view(chat), agent_tree: listing });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click();
}

const mod = (info: TestInfo) => info.project.name === "Windows" ? "Control" : "Meta";

test("a chat with threads lists them in place of the sidebar, and ⌘E lays the sidebar over them", async ({ page }, info) => {
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

  // ⌘E lays the sidebar over the threads, and takes it away again.
  await page.keyboard.press(`${mod(info)}+e`);
  await expect(page.locator(".cwin")).toHaveClass(/rail-on/);
  await expect(page.locator(".cwin")).toHaveClass(/side-over/);
  await expect(page.locator("#side").getByRole("button", { name: "Home", exact: true })).toBeVisible();
  await page.keyboard.press(`${mod(info)}+e`);
  await expect(page.locator(".cwin")).not.toHaveClass(/side-over/);
  await expect(page.locator(".asst-list")).toBeHidden();
});

test("Esc beside a chat's threads shows the sidebar and chats together, then goes straight to Home", async ({ page }) => {
  await openChat(page, [chat, research, notes]);
  const win = page.locator(".cwin");
  await expect(win).toHaveClass(/rail-on/);
  await page.locator("#agmsg").focus();

  await page.keyboard.press("Escape");
  await expect(win).not.toHaveClass(/rail-on/);
  await expect(page.locator("#side")).toBeVisible();
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.getByText("On it.")).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.locator("#title")).toHaveText("Home");
  await expect(page.locator("#side").getByRole("button", { name: "Home", exact: true })).toHaveAttribute("aria-current", "true");
  await expect(page.locator("#agmsg")).toBeHidden();
});

test("Esc from a threaded chat opened over a project reaches Home in two presses instead of returning to the project", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat, research, notes], agent_view: view(chat) });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  await expect(page.locator("#title")).toContainText(projectName);

  await page.evaluate(row => window.__navigation.open({ agent: { kind: "agent", ...row } }), chat);
  await expect(page.locator(".cwin")).toHaveClass(/rail-on/);
  await page.locator("#agmsg").focus();

  await page.keyboard.press("Escape");
  await expect(page.locator("#side")).toBeVisible();
  await expect(page.locator(".asst-list")).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.locator("#title")).toHaveText("Home");
  await expect(page.locator("#side").getByRole("button", { name: "Home", exact: true })).toHaveAttribute("aria-current", "true");
});

test("a detached chat keeps its full height after pressing Escape twice", async ({ page }) => {
  await openDesktop(page, false, {
    center_window_alone: { kind: "chat", machine_id: chat.machine_id, session: chat.id },
    agent_hosts: [host], agent_sessions: [chat, research], agent_view: view(chat),
  }, "chat-sample");
  await expect(page.locator("#rail")).toBeVisible();
  await page.locator("#agmsg").focus();

  const main = page.locator("#main");
  const height = await main.evaluate(el => el.getBoundingClientRect().height);
  for (let press = 0; press < 2; press++) {
    await page.keyboard.press("Escape");
    await expect(page.locator(".asst-list")).toBeVisible();
    await expect(page.getByText("On it.")).toBeVisible();
    await expect(page.locator(".cc-top")).toHaveCSS("padding-left", "84px");
    await expect.poll(() => main.evaluate(el => el.getBoundingClientRect().height)).toBe(height);
  }
});

test("the sidebar over a chat's threads goes with Esc, a click beside it or another page", async ({ page }, info) => {
  await openChat(page, [chat, research, notes]);
  const win = page.locator(".cwin");
  await page.locator("#agmsg").focus();

  await page.keyboard.press(`${mod(info)}+e`);
  await expect(win).toHaveClass(/side-over/);
  await page.keyboard.press("Escape");
  await expect(win).not.toHaveClass(/side-over/);
  await expect(win).toHaveClass(/rail-on/);

  await page.keyboard.press(`${mod(info)}+e`);
  await page.getByText("On it.").click();
  await expect(win).not.toHaveClass(/side-over/);

  await page.keyboard.press(`${mod(info)}+e`);
  await page.locator("#side").getByRole("button", { name: "Home", exact: true }).click();
  await expect(win).not.toHaveClass(/side-over/);
  await expect(page.locator("#side")).toBeVisible();
});

test("a thread started as a persona shows it on its row and in its header", async ({ page }) => {
  const researcher = { ...research, persona: { name: "researcher", color: "blue" } };
  await openChat(page, [chat, researcher, notes]);
  const row = page.locator("#rail .rail-threads .pj-li");

  await expect(row).toHaveText(/Pricing research.*researcher.*Reading the price list/);
  await expect(row.locator(".th-persona")).toHaveText("researcher");
  await expect(page.locator("#rail .rail-scroll > .pj-li .th-persona")).toHaveCount(0);

  await page.evaluate(next => window.__navigation.respond("agent_view", next), view(researcher));
  await row.click();
  await expect(page.locator(".aglocation .th-persona")).toHaveText("researcher");

  // The chats list Esc shows says so too.
  await page.evaluate(next => window.__navigation.respond("agent_view", next), view(chat));
  await page.locator("#agmsg").focus();
  await page.keyboard.press("Escape");
  await expect(page.locator(".aglocation .th-persona")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.locator(".asst-list .asst-threads .pj-li").filter({ hasText: "Pricing research" }).locator(".th-persona")).toHaveText("researcher");
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
    agent_tree: { folders: [{ path: "", entries: [entry("pricing", true), entry("notes", true), entry("drafts", true), entry("notes.md", false)], more: 0 }], truncated: false },
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
    list_machines: [{ id: "build-box", name: "build-box", online: true, seen_at: 0, version: "", env: null }, { id: "other", name: "Other box", online: true, seen_at: 0, version: "", env: null }],
    agent_sessions: [chat], agent_view: view(chat), session_moves: [record],
  });
  const push = (over: object) => page.evaluate(views => window.__navigation.notify("moves", views), [{ command: "session_move_progress", args: { session: "chat-one" }, value: progress(over) }]);
  await push({});
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click();
  await expect(page.locator("#agmovebox")).toContainText("Waiting for Other box");

  // Only the target knows how far it is: its progress comes with the moves push, and nothing asks for it.
  await push({ phase: "files", total: 1_000_000, files_copied: 120_000_000, files_total: 551_000_000 });
  await expect(page.locator("#agmovebox")).toContainText("Files 120.0 MB of 551.0 MB · 22%");
  await expect(page.locator("#agfield").getByRole("progressbar", { name: "Moving to Other box" })).toHaveAttribute("aria-valuenow", "22");
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "session_move_progress").length)).toBe(0);
});

test("a file opened from a chat's Files sits beside the chat, and Markdown opens in its preview", async ({ page }) => {
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat], agent_view: view(chat),
    agent_tree: tree(entry("notes.md")),
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
    agent_tree: tree(entry(".claude", true)),
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

test("chat and thread activity updates badges without reordering chats", async ({ page }) => {
  const asks = { ...research, id: "thread-asks", title: "Budget check", status: "asking", last_activity: 50 };
  const other = { ...chat, id: "chat-two", title: "Write the post", started_at: 4, last_activity: 9 };
  await openDesktop(page, false, { agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat, other, research], agent_view: view(chat) });
  const link = page.locator("#side").getByRole("button", { name: /^Assistant/ });
  await link.click();

  const list = page.locator(".asst-list");
  const chats = list.getByRole("button", { name: /Plan the launch|Write the post/ });
  await expect(chats).toHaveText([/Write the post/, /Plan the launch/]);

  await page.evaluate(rows => {
    window.__navigation.respond("agent_sessions", rows);
    window.__navigation.notify("home", [{ command: "agent_sessions", value: rows }]);
  }, [other, { ...chat, last_activity: 25 }, research, asks]);
  await expect(link).toContainText("1");
  await expect(chats).toHaveText([/Write the post/, /Plan the launch.*1 thread working.*thread needs you/]);
});

test("moving the assistant pane also moves Changes when opening a project session", async ({ page }) => {
  await page.setViewportSize({ width: 1800, height: 900 });
  await page.addInitScript(() => localStorage.setItem("beep.mediaPane.window", JSON.stringify({
    dock: "right", tabs: { changes: "right" },
  })));
  const project = { ...chat, id: "project-session", assistant: false, cwd: "/srv/project" };
  await openChat(page, [chat, project], tree(entry("notes.txt")));
  await expect(page.getByRole("tab", { name: "Changes", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Dock these tabs at the bottom" }).click();
  await page.evaluate(row => {
    window.__navigation.respond("agent_view", row.view);
    window.__navigation.open({ agent: { kind: "agent", ...row.session } });
  }, { session: project, view: view(project) });
  await expect(page.locator("#agpane-bottom").getByRole("tab", { name: "Changes" })).toBeVisible();
  await expect(page.locator("#agpane-right")).toBeHidden();
});

test("a chat's pane folded once opens beside the chat again", async ({ page }) => {
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat], agent_view: view(chat),
    agent_tree: tree(entry("notes.txt"), entry("plan.txt")),
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

test("an empty chat shows its right pane when a thread starts and hides it when the thread ends", async ({ page }) => {
  await openChat(page, [chat, notes], tree(entry("notes", true)));
  const rail = page.locator("#rail");
  const pane = page.locator("#agpane-right");
  await expect(rail.locator(".rail-scroll > .pj-li")).toContainText("Competitor notes");
  await expect(pane.getByText("This folder is empty.")).toHaveCount(1);
  await expect(pane).toBeHidden();

  const sessions = (rows: (typeof chat)[]) => page.evaluate(list => {
    window.__navigation.respond("agent_sessions", list);
    window.__navigation.notify("home", [{ command: "agent_sessions", value: list }]);
  }, rows);
  await sessions([chat, notes, research]);
  await expect(rail.locator(".rail-threads .pj-li")).toHaveText([/Pricing research/]);
  await expect(pane).toBeVisible();

  await sessions([chat, notes, { ...research, status: "ended" }]);
  await expect(pane).toBeHidden();
});

test("files show and hide an empty chat's right pane while a command is running", async ({ page }, info) => {
  await page.setViewportSize({ width: 1800, height: 900 });
  await openChat(page, [chat]);
  const pane = page.locator("#agpane-right");
  const strip = page.locator("#agpstrip-right");
  const topic = "files.%2Fsrv%2F%2Ebeep%2Fassistant%2Fchat-one";
  await expect.poll(() => page.evaluate(topic => window.__navigation.calls().some(call => call.command === "mesh_topics" && JSON.stringify(call.args.topics).includes(topic)), topic)).toBe(true);
  await page.evaluate(id => window.__navigation.processes([{ pid: 41, command: "Read notes", started_at: 1, ports: [], left_group: false, parent: 0, pages: [] }], id), chat.id);
  await expect(pane).toBeHidden();
  await expect(strip).toBeHidden();

  const update = (listing: ReturnType<typeof tree>) => page.evaluate(({ listing, topic }) => {
    window.__navigation.respond("agent_tree", listing);
    window.__navigation.publish(topic, null);
  }, { listing, topic });
  await update(tree(entry("summary.txt")));
  await expect(pane.getByText("summary.txt", { exact: true })).toBeVisible();
  await expect(pane.getByRole("tab", { name: /Running/ })).toBeVisible();

  await pane.getByRole("textbox", { name: "Filter files" }).fill("Missing filename");
  await expect(pane.getByText("No files match.")).toBeVisible();
  await update(tree());
  await expect(pane).toBeHidden();
  await expect(strip).toBeHidden();

  await page.keyboard.press(`${mod(info)}+2`);
  await expect(pane.getByRole("tab", { name: "Files" })).toBeVisible();
});

test("a chat picks library skills for its folder and its threads", async ({ page }) => {
  const skills = [{ name: "pdf", description: "Reads PDFs." }, { name: "notes", description: "Keeps notes." }];
  const tools = { name: "tools", url: "https://git.example.com/tools.git", revision: "a1", skills, global: "None", projects: {}, chats: {} };
  await openDesktop(page, false, { agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: [chat], agent_view: view(chat), skill_libraries_list: [tools], chat_skills_set: null });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();

  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Skills" }).click();
  const sheet = page.getByRole("dialog", { name: "Skills in Plan the launch" });
  await expect(sheet).toContainText("in this chat's folder and each of its threads'");
  await expect(sheet.getByText("Keep out of Git")).toHaveCount(0);
  await sheet.getByRole("checkbox", { name: "tools" }).check();
  await sheet.getByRole("button", { name: "Save" }).click();

  await expect(sheet).toBeHidden();
  const saved = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "chat_skills_set").map(call => call.args));
  expect(saved).toEqual([{ chat: "chat-one", picks: { tools: "All" } }]);
});

test("a restored chat loads its transcript and HTML preview after reconnecting", async ({ page }) => {
  const label = "chat-restored";
  const paneKey = `beep.projectView.${chat.machine_id}\n${chat.id}:${label}`;
  await page.addInitScript(({ paneKey, folder }) => {
    window.__BEEP_RESTORING_WINDOW__ = true;
    if (!localStorage.getItem(paneKey)) localStorage.setItem(paneKey, JSON.stringify({
      tabs: [{ root: folder, rel: "report.html", preview: true }], on: { right: "text:report.html" },
    }));
  }, { paneKey, folder });
  await openDesktop(page, false, {
    center_window_alone: { kind: "chat", machine_id: chat.machine_id, session: chat.id },
    agent_sessions: [], agent_view: view(chat),
    agent_tree_text: { size: 12, text: "<p>Hello</p>", cut: false, binary: false },
    agent_artifact: { kind: "html", src: "data:text/html,%3Cp%3EHello%3C%2Fp%3E" },
  }, label);

  const waiting = page.getByText("Waiting for the machine to list this chat…");
  await expect(waiting).toBeVisible();
  const reconnect = () => page.evaluate(row => {
    window.__navigation.respond("agent_sessions", [row]);
    window.__navigation.emit("reconnected", null);
  }, chat);
  await page.evaluate(() => window.__navigation.rejectOnce("agent_tree_text", "Connection interrupted"));
  await reconnect();
  await expect(page.locator("#agbody")).toContainText("On it.");
  await expect(page.getByText("Connection interrupted", { exact: true })).toBeVisible();
  await expect(page.locator("#agtitle")).toHaveText(chat.title);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_view" || call.command === "agent_follow"));
  expect(reads.length).toBeGreaterThan(0);
  expect(reads.every(call => call.args.agent === chat.agent && call.args.session === chat.id && call.args.machineId === chat.machine_id)).toBe(true);
  const savedTabs = () => page.evaluate(key => JSON.parse(localStorage.getItem(key) ?? "{}").tabs, paneKey);
  expect(await savedTabs()).toEqual([{ root: folder, rel: "report.html", preview: true }]);

  await reconnect();
  const preview = page.frameLocator('iframe[title="Preview of report.html"]').locator("body");
  await expect(preview).toHaveText("Hello");

  await page.getByRole("button", { name: "Source", exact: true }).click();
  expect(await savedTabs()).toEqual([{ root: folder, rel: "report.html", preview: false }]);
  await page.reload();
  await expect(waiting).toBeVisible();
  await reconnect();
  await expect(page.locator(".agcode:visible")).toContainText("<p>Hello</p>");
});

test("a restored thread stays selected when the parent chat connects first", async ({ page }) => {
  const label = "chat-thread";
  await page.addInitScript(({ label, machine, thread }) => {
    window.__BEEP_RESTORING_WINDOW__ = true;
    localStorage.setItem(`beep.windowSession:${label}`, JSON.stringify([machine, thread]));
  }, { label, machine: research.machine_id, thread: research.id });
  await openDesktop(page, false, {
    center_window_alone: { kind: "chat", machine_id: chat.machine_id, session: chat.id },
    agent_sessions: [chat], agent_view: view(research),
  }, label);
  await expect(page.getByRole("button", { name: "Open parent chat" })).toBeVisible();
  await page.evaluate(rows => {
    window.__navigation.respond("agent_sessions", rows);
    window.__navigation.notify("home", [{ command: "agent_sessions", value: rows }]);
  }, [chat, research]);
  await expect(page.locator("#agtitle")).toHaveText(research.title);
  await expect(page.locator("#agbody")).toContainText("On it.");
});
