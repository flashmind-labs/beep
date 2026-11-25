import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("settings redraw preserves the agent account input and closes its root", async ({ page }) => {
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    agent_accounts: { claude: null, codex_key: null, machines: [], named: [] },
    list_machines: [], list_credentials: [], list_github_accounts: [],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agents", exact: true }).click();
  await settings.getByRole("button", { name: "Paste a token" }).click();
  const input = settings.getByRole("textbox", { name: "Claude Code token" });
  await input.focus();
  await input.evaluate(el => { Reflect.set(window, "agentHost", el.closest("#set-agents")); });
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(1);
  await expect(input).toBeFocused();
  await input.fill("example-secret");
  await input.evaluate(el => { if (el instanceof HTMLInputElement) el.setSelectionRange(2, 5); });
  await page.evaluate(() => window.__navigation.hold("save_agent_token"));
  await input.press("Enter");
  await expect(settings.getByRole("button", { name: "Saving…", exact: true })).toBeDisabled();
  await expect(input).toBeFocused();
  await expect(input).toHaveValue("example-secret");
  await expect(input).toHaveJSProperty("selectionStart", 2);
  await expect(input).toHaveJSProperty("selectionEnd", 5);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "save_agent_token" && call.args.agentSlug === "claude" && call.args.secret === "example-secret"))).toBe(true);
  await page.evaluate(() => window.__navigation.release("save_agent_token"));
  await expect(input).toHaveCount(0);
  const edges = await settings.evaluate(sheet => {
    const buttons = [...sheet.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [href], [tabindex]:not([tabindex='-1'])")];
    buttons.at(-1)?.focus();
    return buttons.length;
  });
  expect(edges).toBeGreaterThan(0);
  await page.keyboard.press("Tab");
  expect(await settings.evaluate(sheet => document.activeElement === sheet.querySelector("button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [href], [tabindex]:not([tabindex='-1'])"))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => { const host: unknown = Reflect.get(window, "agentHost"); return host instanceof HTMLElement ? host.isConnected : null; })).toBe(false);
});

test("a completed save does not discard a newer token draft", async ({ page }) => {
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    agent_accounts: { claude: null, codex_key: null, machines: [], named: [] },
    list_machines: [], list_credentials: [], list_github_accounts: [],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agents", exact: true }).click();
  await settings.getByRole("button", { name: "Paste a token" }).click();
  await page.evaluate(() => window.__navigation.hold("save_agent_token"));
  await settings.getByRole("textbox", { name: "Claude Code token" }).fill("first-secret");
  await settings.getByRole("textbox", { name: "Claude Code token" }).press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "save_agent_token"))).toBe(true);
  await settings.getByRole("button", { name: "Cancel", exact: true }).click();
  await settings.getByRole("button", { name: "Paste a token" }).click();
  const newer = settings.getByRole("textbox", { name: "Claude Code token" });
  await newer.fill("new-secret");
  await page.evaluate(() => window.__navigation.release("save_agent_token"));
  await expect(settings.getByRole("button", { name: "Save", exact: true })).toBeEnabled();
  await expect(newer).toHaveValue("new-secret");
});

test("Claude signs in on the selected machine and refreshes its account", async ({ page }) => {
  const machines = [
    { id: "build-box", name: "build-box", online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
    { id: "test-box", name: "test-box", online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
    { id: "offline-box", name: "offline-box", online: false, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
    { id: "unknown-box", name: "unknown-box", online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
  ];
  const login = (id: string, signed_in: boolean, account: string | null) => ({ id, env: { logins: [{ tool: "claude", signed_in, account }] } });
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    agent_accounts: { claude: null, codex_key: null, machines, named: [] },
    list_machines: [login("build-box", true, "one@example.com"), login("test-box", false, null)],
    list_credentials: [], list_github_accounts: [],
    machine_sign_in: { state: "signed_in", url: null, code: null, asks_code: false, reason: null },
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agents", exact: true }).click();
  const row = (name: string) => settings.locator(".dline.dsm.dpy").filter({ has: page.getByText(name, { exact: true }) });
  await expect(row("build-box")).toContainText("signed in as one@example.com");
  await expect(row("build-box").locator(".ddot.ok")).toHaveCount(1);
  await expect(row("test-box").locator(".ddot.warn")).toHaveCount(1);
  await expect(row("offline-box").locator(".ddot.off")).toHaveCount(1);
  await expect(row("build-box").getByRole("button")).toHaveCount(0);
  await expect(row("offline-box").getByRole("button")).toHaveCount(0);
  await expect(row("unknown-box").getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
  await row("test-box").getByRole("button", { name: "Sign in", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Sign in to Claude Code on test-box", exact: true });
  await expect(sheet).toContainText("Claude Code is signed in on test-box.");
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "machine_sign_in").map(call => call.args))).toContainEqual({ machineId: "test-box", tool: "claude", step: { step: "start" } });
  const count = (command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).length, command);
  const stateReads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "machine_sign_in" && (call.args.step as { step: string }).step === "state").length);
  const accountReads = await count("agent_accounts");
  // The node says how the sign-in stands; the sheet draws it without asking.
  // The sheet's subscription reaches native code a moment after it opens, so the update repeats until drawn.
  await expect(async () => {
    await page.evaluate(() => window.__navigation.publish("sign-in.claude", null, { SignIn: { Failed: { reason: "The sign-in timed out" } } }, "test-box"));
    await expect(sheet).toContainText("The sign-in timed out", { timeout: 300 });
  }).toPass();
  expect(await stateReads()).toBe(0);
  await page.evaluate(machines => window.__navigation.notify("machines", [{ command: "list_machines", value: machines }]), [login("build-box", true, "one@example.com"), login("test-box", true, "two@example.com")]);
  await sheet.getByRole("button", { name: "Close", exact: true }).click();
  await expect(row("test-box")).toContainText("signed in as two@example.com");
  await expect(row("test-box").getByRole("button")).toHaveCount(0);
  await expect(row("build-box")).toContainText("signed in as one@example.com");
  expect(await page.evaluate(() => window.__navigation.calls().some(call => call.command === "claude_sign_in" || call.command === "save_agent_token"))).toBe(false);
  // Closing the sheet reads no accounts: the accounts update carries a changed sign-in.
  expect(await count("agent_accounts")).toBe(accountReads);
});

test("Antigravity signs in on the machine that is signed out", async ({ page }) => {
  const login = (id: string, online: boolean, signed_in: boolean) => ({ id, name: id, online, env: { logins: [{ tool: "agy", signed_in, account: null }] } });
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    agent_accounts: { claude: null, codex_key: null, machines: [], named: [] },
    list_machines: [login("build-box", true, true), login("test-box", true, false), login("offline-box", false, false)],
    list_credentials: [], list_github_accounts: [],
    machine_sign_in: { state: "waiting", url: "https://accounts.google.com/o/oauth2/auth?state=xyz", code: null, asks_code: true, reason: null },
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agents", exact: true }).click();
  const row = (name: string) => settings.locator(".dline.dsm.dpy").filter({ has: page.getByText(name, { exact: true }) });
  await expect(row("build-box")).toContainText("signed in");
  await expect(row("build-box").getByRole("button")).toHaveCount(0);
  await expect(row("offline-box").getByRole("button")).toHaveCount(0);
  await row("test-box").getByRole("button", { name: "Sign in", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Sign in to Antigravity on test-box", exact: true });
  await expect(sheet).toContainText("accounts.google.com/o/oauth2/auth");
  await sheet.getByRole("textbox", { name: "Code from the page" }).fill("4/0code");
  await sheet.getByRole("button", { name: "Send", exact: true }).click();
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "machine_sign_in").map(call => call.args))).toContainEqual({ machineId: "test-box", tool: "agy", step: { step: "answer", text: "4/0code" } });
});

test("a named account is added beside the default one", async ({ page }) => {
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    agent_accounts: { claude: null, codex_key: null, machines: [], named: [] },
    list_machines: [], list_credentials: [], list_github_accounts: [],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agents", exact: true }).click();
  const claude = settings.locator(".dbox").filter({ has: page.locator("b.dtitle", { hasText: /^Claude Code$/ }) });
  await claude.getByRole("button", { name: "Add account" }).click();
  await expect(claude.locator("#agNamedSign")).toBeVisible();
  await expect(claude.getByRole("textbox", { name: "Claude Code token" })).toHaveCount(0);
  await claude.locator("#agNamedPaste").click();
  await expect(claude.getByRole("textbox", { name: "Claude Code token" })).toBeVisible();
  const codex = settings.locator(".dbox").filter({ has: page.locator("b.dtitle", { hasText: /^Codex$/ }) });
  await codex.getByRole("button", { name: "Add account" }).click();
  await settings.getByRole("textbox", { name: "Account name" }).fill("default");
  await settings.getByRole("textbox", { name: "Codex access token" }).fill("example-token");
  await settings.locator("#agNamedSave").click();
  await expect(settings.locator("#agNamedErr")).toContainText("pick another name");
  await settings.getByRole("textbox", { name: "Account name" }).fill(" Work ");
  await settings.locator("#agNamedSave").click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "save_agent_token").map(call => call.args))).toEqual([{ agentSlug: "codex", name: "Work", secret: "example-token" }]);
});

test("a token saved here shows from the accounts push, without reading the accounts again", async ({ page }) => {
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    agent_accounts: { claude: null, codex_key: null, machines: [], named: [] },
    list_machines: [], list_credentials: [], list_github_accounts: [], save_agent_token: null,
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agents", exact: true }).click();
  await settings.getByRole("button", { name: "Paste a token" }).click();
  await settings.getByRole("textbox", { name: "Claude Code token" }).fill("example-secret");
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_accounts").length);
  await expect.poll(reads).toBeGreaterThan(0);
  const before = await reads();

  await settings.getByRole("textbox", { name: "Claude Code token" }).press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "save_agent_token"))).toBe(true);
  await expect(settings.getByRole("textbox", { name: "Claude Code token" })).toHaveCount(0);
  expect(await reads()).toBe(before);

  const token = { last4: "cret", created_at: 1, unreadable: false };
  await page.evaluate(token => window.__navigation.notify("accounts", [{ command: "agent_accounts", value: { claude: token, codex_key: null, machines: [], named: [] } }]), token);
  await expect(settings.getByText("…cret")).toBeVisible();
  expect(await reads()).toBe(before);
});

test("a Codex sign-in is asked about only once its task has ended, from the processes update", async ({ page }) => {
  const machines = [{ id: "test-box", name: "test-box", online: true, claude: false, codex: true, claude_sessions: 0, codex_signed_in: false }];
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    agent_accounts: { claude: null, codex_key: null, machines, named: [] },
    list_machines: [], list_credentials: [], list_github_accounts: [],
    codex_login_start: { task: 4, url: "https://auth.openai.com/codex/device", code: "ABCD-12345", expires_at: Date.now() + 900_000 },
    codex_login_state: { state: "waiting", message: null },
    open_url: null,
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agents", exact: true }).click();
  await settings.locator(".dbox").filter({ has: page.locator("b.dtitle", { hasText: /^Codex$/ }) }).getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(settings).toContainText("ABCD-12345");
  const asked = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "codex_login_state").length);
  const tasks = (state: unknown) => ({ Process: { Tasks: { tasks: [{ id: 4, command: "codex login --device-auth", state }] } } });
  const before = await asked();

  // While the task runs, its updates say so and nothing is asked.
  for (const revision of [7000, 7001]) await page.evaluate(([data, at]) => window.__navigation.publish("processes.*", at, data, "test-box"), [tasks("running"), revision] as const);
  await expect(settings).toContainText("ABCD-12345");
  expect(await asked()).toBe(before);

  // Once it ended, the state is read once and the row follows it.
  await page.evaluate(() => window.__navigation.respond("codex_login_state", { state: "signed_in", message: null }));
  await expect(async () => {
    await page.evaluate(data => window.__navigation.publish("processes.*", null, data, "test-box"), tasks({ exited: { code: 0 } }));
    await expect(settings).not.toContainText("ABCD-12345", { timeout: 300 });
  }).toPass();
  expect(await asked()).toBeGreaterThan(before);
});
