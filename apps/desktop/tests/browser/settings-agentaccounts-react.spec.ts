import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("settings redraw preserves the agent account input and closes its root", async ({ page }) => {
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    agent_accounts: { claude: null, codex_key: null, machines: [] },
    list_machines: [], list_credentials: [], list_github_accounts: [],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agent accounts", exact: true }).click();
  await settings.getByRole("button", { name: "Paste a token" }).click();
  const input = settings.getByRole("textbox", { name: "Claude Code token" });
  await input.focus();
  await input.evaluate(el => { Reflect.set(window, "agentInput", el); Reflect.set(window, "agentHost", el.closest("#set-agents")); });
  // Coming back to the window redraws Settings while no token is typed.
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(1);
  expect(await input.evaluate(el => el === Reflect.get(window, "agentInput") && document.activeElement === el)).toBe(true);
  await input.fill("example-secret");
  await input.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "save_agent_token" && call.args.agentSlug === "claude" && call.args.secret === "example-secret"))).toBe(true);
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
    agent_accounts: { claude: null, codex_key: null, machines: [] },
    list_machines: [], list_credentials: [], list_github_accounts: [],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agent accounts", exact: true }).click();
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
    agent_accounts: { claude: null, codex_key: null, machines },
    list_machines: [login("build-box", true, "one@example.com"), login("test-box", false, null)],
    list_credentials: [], list_github_accounts: [],
    machine_sign_in: { state: "signed_in", url: null, code: null, asks_code: false, reason: null },
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agent accounts", exact: true }).click();
  const row = (name: string) => settings.locator(".dline.dsm.dpy").filter({ has: page.getByText(name, { exact: true }) });
  await expect(row("build-box")).toContainText("signed in as one@example.com");
  await expect(row("build-box").getByRole("button")).toHaveCount(0);
  await expect(row("offline-box").getByRole("button")).toHaveCount(0);
  await expect(row("unknown-box").getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
  await row("test-box").getByRole("button", { name: "Sign in", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Sign in to Claude Code on test-box", exact: true });
  await expect(sheet).toContainText("Claude Code is signed in on test-box.");
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "machine_sign_in").map(call => call.args))).toContainEqual({ machineId: "test-box", tool: "claude", step: { step: "start" } });
  await page.evaluate(machines => window.__navigation.respond("list_machines", machines), [login("build-box", true, "one@example.com"), login("test-box", true, "two@example.com")]);
  await sheet.getByRole("button", { name: "Close", exact: true }).click();
  await expect(row("test-box")).toContainText("signed in as two@example.com");
  await expect(row("test-box").getByRole("button")).toHaveCount(0);
  await expect(row("build-box")).toContainText("signed in as one@example.com");
  expect(await page.evaluate(() => window.__navigation.calls().some(call => call.command === "claude_sign_in" || call.command === "save_agent_token"))).toBe(false);
});
