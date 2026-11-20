import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const host = { machine_id: "build-box", machine_name: "build-box", agents: ["codex", "flsh"], workspace: null, home: "/srv", capabilities: [], assistant: true, hermes: false };
const session = { machine_id: "build-box", machine_name: "build-box", id: "assistant-one", agent: "flsh", title: null as string | null, cwd: "/srv", status: "working", doing: null as string | null, started_at: 1, last_activity: 1, external: false, question: null, assistant: true };

const view = (row: typeof session, text: string) => ({ session: row, plan: [], files: [], entries: [{ kind: "agent", text }], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 1 });

/// The start this device keeps for a chat, and its page: the message alone.
const start = { ...session, id: "start:one", status: "starting", doing: "Starting flsh on build-box" };
const kept = { agent_begin: { session: start, why: "" }, agent_view: { ...view(start, ""), entries: [{ kind: "user", text: "hello" }], turns: 0 } };

/// The start's machine names its session `row`, which answers "Hello.".
const named = (page: Page, row: typeof session) => page.evaluate(event => {
  window.__navigation.respond("agent_view", event);
  window.__navigation.emit("agent-started", { start: "start:one", session: event.session, first_message: "hello" });
}, view(row, "Hello."));

test("the sidebar's Assistant lists the chats and opens one beside them", async ({ page }) => {
  const other = { ...session, id: "assistant-two", title: "Check the open pull requests", status: "idle" };
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null },
    agent_sessions: [{ ...session, title: "Set up the widget project" }, other],
    agent_view: view(other, "Two pull requests wait for review."),
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  const list = page.locator(".asst-list");
  await expect(list.locator(".pj-li")).toHaveCount(2);
  await expect(page.locator("#agprompt")).toHaveAttribute("placeholder", "Ask the assistant.");

  await list.getByRole("button", { name: /Check the open pull requests/ }).click();
  await expect(page.getByText("Two pull requests wait for review.")).toBeVisible();
  await expect(list).toBeVisible();
  await expect(list.locator('[aria-current="true"]')).toContainText("Check the open pull requests");
  // One header for the page, and no files, changes or terminals beside an assistant chat.
  await expect(page.locator("#title")).toContainText("Assistant");
  await expect(page.locator("#agpane-right")).toBeHidden();
  await expect(page.locator("#agpstrip-right")).toBeHidden();
  await expect(page.locator("#side").getByRole("button", { name: "Assistant", exact: true })).toHaveAttribute("aria-current", "true");
});

test("a file an assistant chat links to opens over the window, read from its machine", async ({ page }) => {
  const reply = "Read [notes.txt](notes.txt), then play [a.wav](tts-samples/a.wav).";
  const done = { ...session, status: "idle" };
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null },
    agent_sessions: [{ ...done, title: "Make voice samples" }], agent_view: { ...view(done, reply), entries: [{ kind: "agent", text: reply, html: '<p>Read <a href="notes.txt">notes.txt</a>, then play <a href="tts-samples/a.wav">a.wav</a>.</p>' }] },
    agent_tree_text: { size: 6, text: "hello\n", cut: false, binary: false, editable: true },
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Make voice samples/ }).click();
  const reads = (command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => call.args), command);

  await page.locator("#agbody").getByRole("link", { name: "notes.txt" }).click();
  const viewer = page.getByRole("dialog", { name: "notes.txt" });
  await expect(viewer.locator("pre")).toHaveText("hello\n");
  expect(await reads("agent_tree_text")).toEqual([{ machineId: "build-box", root: "/srv", path: "notes.txt" }]);
  await page.keyboard.press("Escape");
  await expect(viewer).toBeHidden();

  await page.evaluate(() => window.__navigation.respond("agent_file", { size: 1, mime: "audio/wav", data: "AA==" }));
  await page.locator("#agbody").getByRole("link", { name: "a.wav" }).click();
  await expect(page.getByRole("dialog", { name: "a.wav" }).locator("audio")).toHaveCount(1);
  expect(await reads("agent_file")).toEqual([{ machineId: "build-box", path: "/srv/tts-samples/a.wav", offset: 0, session: "assistant-one" }]);
});

test("Cmd+N on Home opens the Assistant page and starts the assistant with its harness in the home folder", { tag: "@windows" }, async ({ page }, info) => {
  await openDesktop(page, false, {
    agent_hosts: [host],
    assistant_settings_get: { agent: "flsh", model: null, effort: "high", fast: null },
    ...kept,
  });
  await expect(page.locator("#sec-running")).toBeVisible();
  await page.keyboard.press(info.project.name === "Windows" ? "Control+n" : "Meta+n");

  const prompt = page.locator("#agprompt");
  await expect(prompt).toBeFocused();
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#title")).toContainText("Assistant");
  await expect(page.locator("#agpillb .flsh")).toBeVisible();
  await expect(page.locator("#agpillb")).toContainText("High");
  await expect(page.locator("#aghostb")).toHaveText("build-box");
  await expect(page.locator("#agfoldb")).toBeHidden();

  await prompt.fill("How is the widget project doing?");
  await prompt.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").map(call => call.args))).toEqual([
    // No machine picked: Auto picks one, this computer first.
    { machineId: null, agent: "flsh", cwd: null, prompt: "How is the widget project doing?", images: [], model: null, effort: "high", fast: null, checkout: null, assistant: true, account: null },
  ]);
  // The new chat opens on the page, picked in its list.
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#agshell")).toBeVisible();
});

test("Cmd+L opens the Assistant page from anywhere in the window", { tag: "@windows" }, async ({ page }, info) => {
  await openDesktop(page, false, { agent_hosts: [host], agent_sessions: [session], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null } });
  await page.locator("#side").getByRole("button", { name: "Terminals", exact: true }).click();
  await page.keyboard.press(info.project.name === "Windows" ? "Control+l" : "Meta+l");

  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#agprompt")).toBeFocused();
  await expect(page.locator("#side").getByRole("button", { name: "Assistant", exact: true })).toHaveAttribute("aria-current", "true");
});

test.describe("on a phone", () => {
  test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

  test("New assistant on Home opens the chats, and New chat the assistant's start form", async ({ page }) => {
    await openDesktop(page, false, { agent_hosts: [host], agent_sessions: [session], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null } });
    await expect(page.locator("body")).toHaveClass(/mobile/);
    await page.getByRole("button", { name: "New assistant" }).tap();
    const list = page.locator(".asst-list");
    await expect(list.locator(".pj-li")).toHaveCount(1);
    await expect(page.locator("#agprompt")).toBeHidden();
    await list.getByRole("button", { name: "New chat" }).tap();
    await expect(page.locator("#agprompt")).toBeVisible();
    await expect(list).toBeHidden();
    await expect(page.locator("#agpillb .flsh")).toBeVisible();
    await expect(page.locator("#agfoldb")).toBeHidden();
  });
});

test("the first start's harness shows the assistant setting saved in Settings", async ({ page }) => {
  const models = { pick: "listed", current: null, models: [{ id: "model-b", name: "Example Model B", description: null, provider: null }] };
  await openDesktop(page, false, {
    center_home: { machines: [], projects: [], running: [], needs_you: [] },
    settings: { name: "build-box", version: "0.9.0", autostart: false, agent_notices: false },
    agent_hosts: [host], agent_models: models, mac_control: { id: "build-box" },
    assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null },
  });
  await expect(page.locator(".painted-empty .pill .flsh")).toBeVisible();

  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Assistant", exact: true }).click();
  await settings.locator("#assistantSetting").getByRole("button").click();
  // Saved in Settings: Home reads the setting again.
  await page.evaluate(() => window.__navigation.respond("assistant_settings_get", { agent: "codex", model: null, effort: null, fast: null }));
  await page.getByRole("dialog", { name: "Harness, model and thinking" }).getByText("Example Model B").click();
  await expect(page.locator(".painted-empty .pill .codex")).toBeVisible();
});

test("a chat started from Home's composer opens on the Assistant page", async ({ page }) => {
  await openDesktop(page, false, {
    center_home: { machines: [], projects: [], running: [], needs_you: [] },
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, ...kept,
  });
  const prompt = page.locator(".painted-empty textarea");
  await prompt.fill("hello");
  await prompt.press("Enter");
  await expect(page.locator("#agbody .dcoderen.user")).toContainText("hello");
  await expect(page.locator(".asst-list")).toBeVisible();

  await named(page, session);
  await expect(page.getByText("Hello.", { exact: true })).toBeVisible();
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#title")).toContainText("Assistant");
});

test("Shift+Tab in the assistant's composer moves to the next machine that runs it", async ({ page }) => {
  const other = { ...host, machine_id: "gpu-box", machine_name: "gpu-box" };
  const old = { ...host, machine_id: "old-box", machine_name: "old-box", assistant: false };
  await openDesktop(page, false, { agent_hosts: [host, other, old], agent_sessions: [], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null } });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  const prompt = page.locator("#agprompt");
  await expect(page.locator("#aghostb")).toHaveText("build-box");

  await prompt.press("Shift+Tab");
  await expect(page.locator("#aghostb")).toHaveText("gpu-box");
  await expect(prompt).toBeFocused();
  await prompt.press("Shift+Tab");
  await expect(page.locator("#aghostb")).toHaveText("build-box");
});

test("the chats' grip resizes them and keeps the width", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: [host], agent_sessions: [session], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null } });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  const list = page.locator(".asst-list"), grip = page.getByRole("separator", { name: "Resize the chats" });
  await expect(list).toBeVisible();
  const before = (await list.boundingBox())?.width ?? 0;

  const at = await grip.boundingBox();
  if (!at) throw new Error("The grip is not on screen");
  await page.mouse.move(at.x + at.width / 2, at.y + 200);
  await page.mouse.down();
  await page.mouse.move(at.x + at.width / 2 + 100, at.y + 200, { steps: 4 });
  await page.mouse.up();
  await expect.poll(async () => Math.round((await list.boundingBox())?.width ?? 0)).toBe(Math.round(before + 100));
  await expect.poll(() => page.evaluate(() => localStorage.getItem("beep.chatsWidth"))).toBe(String(Math.round(before + 100)));

  await grip.dblclick();
  await expect.poll(async () => Math.round((await list.boundingBox())?.width ?? 0)).toBe(300);
});

test("a chat started on the Assistant page stays on it with a released node", async ({ page }) => {
  // A released node's session and view leave out that the session is the assistant.
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null },
    agent_sessions: [], ...kept,
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  const prompt = page.locator("#agprompt");
  await prompt.fill("hello");
  await prompt.press("Enter");
  await expect(page.locator("#agconv .dbanner")).toContainText("Starting flsh on build-box");

  await named(page, { ...session, assistant: false });
  await expect(page.getByText("Hello.", { exact: true })).toBeVisible();
  await page.waitForTimeout(1500);
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#title")).toContainText("Assistant");
});

test("a chat started on the Assistant page stays on it while the list also has it as found outside Beep", async ({ page }) => {
  const outside = { ...session, assistant: false, external: true, status: "working" };
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null },
    agent_sessions: [outside], ...kept,
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  const prompt = page.locator("#agprompt");
  await prompt.fill("hello");
  await prompt.press("Enter");
  await expect(page.locator("#agconv .dbanner")).toContainText("Starting flsh on build-box");

  await named(page, session);
  await expect(page.getByText("Hello.", { exact: true })).toBeVisible();
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#title")).toContainText("Assistant");
});

/// Hermes answers on build-box, reached there at its default port.
const hermes = { machine: "build-box", machine_name: "build-box", host: "127.0.0.1", port: 9119, last4: "a1b2" };
const firstStart = { center_home: { machines: [], projects: [], running: [], needs_you: [] }, found_projects: [] };
const settings = { settings: { name: "build-box", version: "0.9.0", autostart: false, agent_notices: false, developer: false }, list_machines: [], list_credentials: [], list_github_accounts: [], skill_sync_get: true, project_sync_get: true };
const begun = (page: Page) => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").map(call => call.args));

test("with Hermes connected the assistant starts on Hermes's machine, whatever the harness pick", async ({ page }) => {
  // A node beside Hermes in Docker offers it and no coding agent.
  await openDesktop(page, false, { ...firstStart, ...kept, agent_hosts: [{ ...host, agents: [], hermes: true }], assistant_settings_get: { agent: "codex", model: "model-a", effort: "high", fast: null, hermes } });
  const prompt = page.getByRole("textbox", { name: "Message for the assistant" });
  await expect(page.locator(".painted-empty .pill .hermes-gateway")).toBeVisible();

  await prompt.fill("hello");
  await prompt.press("Enter");
  await expect.poll(() => begun(page)).toEqual([
    { machineId: "build-box", agent: "hermes-gateway", cwd: null, prompt: "hello", images: [], model: null, effort: null, fast: null, checkout: null, assistant: true, account: null },
  ]);
});

test("Hermes on a machine that does not offer it fails with the way to its settings", async ({ page }) => {
  await openDesktop(page, false, { ...firstStart, ...settings, agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null, hermes } });
  const prompt = page.getByRole("textbox", { name: "Message for the assistant" });

  await prompt.fill("hello");
  await prompt.press("Enter");
  await expect(page.getByText("Hermes is not reachable on build-box.")).toBeVisible();
  expect(await begun(page)).toEqual([]);

  await page.getByRole("button", { name: "Open Assistant settings" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" }).getByText("Hermes on build-box · 127.0.0.1:9119 · token …a1b2")).toBeVisible();
});

test("Connect Hermes says why the dashboard refused", async ({ page }) => {
  await openDesktop(page, false, {
    ...settings, list_machines: [{ id: "build-box", name: "build-box", online: true, can_host: true, seen_at: 0, version: "0.9.0", env: null }],
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null, hermes: null },
    hermes_connect: { state: "refused", machine: "build-box", message: "Hermes refused the token." },
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await dialog.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Assistant", exact: true }).click();

  await dialog.getByRole("textbox", { name: "Token" }).fill("secret-token");
  await dialog.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(dialog.getByRole("alert")).toHaveText("Hermes refused the token.");
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "hermes_connect").map(call => call.args))).toEqual([{ machine: "build-box", host: "127.0.0.1", port: 9119, token: "secret-token" }]);
});
