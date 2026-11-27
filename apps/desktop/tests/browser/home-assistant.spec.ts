import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const host = { machine_id: "build-box", machine_name: "build-box", agents: ["codex", "flsh"], workspace: null, home: "/srv", capabilities: [], assistant: true };
const session = { machine_id: "build-box", machine_name: "build-box", id: "assistant-one", agent: "flsh", title: null as string | null, cwd: "/srv", status: "working", doing: null as string | null, started_at: 1, last_activity: 1, external: false, question: null, assistant: true };

const view = (row: typeof session, text: string) => ({ session: row, plan: [], entries: [{ kind: "agent", text }], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1 });

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
    // The chat's pane stays hidden while its folder is empty.
    agent_tree: { folders: [{ path: "", entries: [{ name: "notes.txt", folder: false, size: 1, ignored: false, change: null }], more: 0, git: false }], truncated: false },
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  const list = page.locator(".asst-list");
  await expect(list.locator(".pj-li")).toHaveCount(2);
  await expect(page.locator("#agprompt")).toHaveAttribute("placeholder", "Ask the assistant.");

  await list.getByRole("button", { name: /Check the open pull requests/ }).click();
  await expect(page.getByText("Two pull requests wait for review.")).toBeVisible();
  await expect(list).toBeVisible();
  await expect(list.locator('[aria-current="true"]')).toContainText("Check the open pull requests");
  // One header for the page, and the chat's files beside it, with no Changes: its folder has no git.
  await expect(page.locator("#title")).toContainText("Assistant");
  await expect(page.getByRole("tab", { name: "Files", exact: true }).first()).toBeVisible();
  await expect(page.getByRole("tab", { name: "Changes", exact: true })).toHaveCount(0);
  await expect(page.locator("#side").getByRole("button", { name: "Assistant", exact: true })).toHaveAttribute("aria-current", "true");
});

test("a file an assistant chat links to opens beside it, a page or Markdown in its preview", async ({ page }) => {
  const reply = "See [notes.txt](notes.txt), [the findings](findings.md) and [the dashboard](19-dashboard/index.html).";
  const done = { ...session, status: "idle" };
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null },
    agent_sessions: [{ ...done, title: "Make the dashboard" }], agent_view: { ...view(done, reply), entries: [{ kind: "agent", text: reply, html: '<p>See <a href="notes.txt">notes.txt</a>, <a href="findings.md">the findings</a> and <a href="19-dashboard/index.html">the dashboard</a>.</p>' }] },
    agent_tree_text: { size: 6, text: "hello\n", cut: false, binary: false, editable: true },
    agent_artifact: { kind: "html", html: "<h1>Dashboard</h1>" },
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Make the dashboard/ }).click();
  const pane = page.locator("#agpane-right");

  await page.locator("#agbody").getByRole("link", { name: "notes.txt" }).click();
  await expect(pane.getByRole("tab", { name: /notes\.txt/ })).toBeVisible();
  await expect(page.getByRole("dialog", { name: "notes.txt" })).toHaveCount(0);

  await page.locator("#agbody").getByRole("link", { name: "the findings" }).click();
  await expect(pane.getByRole("tab", { name: /findings\.md/ })).toBeVisible();
  await expect(pane.getByRole("button", { name: "Source", exact: true })).toBeVisible();

  await page.locator("#agbody").getByRole("link", { name: "the dashboard" }).click();
  await expect(pane.getByRole("button", { name: "Source", exact: true })).toBeVisible();
  await expect(page.locator(".agshell")).not.toHaveClass(/zoomed/);
  await expect(page.getByText("See notes.txt, the findings")).toBeVisible();
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

test("a new chat keeps what was typed when the user leaves before sending", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: [host], agent_sessions: [session], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null } });
  const assistant = page.locator("#side").getByRole("button", { name: "Assistant", exact: true });
  await assistant.click();
  await page.locator("#agprompt").fill("Check the widget project");

  await page.locator("#side").getByRole("button", { name: "Home", exact: true }).click();
  await expect(page.locator("#agprompt")).toBeHidden();
  await assistant.click();

  await expect(page.locator("#agprompt")).toHaveValue("Check the widget project");
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
