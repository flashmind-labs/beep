import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("settings redraw preserves the agent account input and closes its root", async ({ page }) => {
  await openDesktop(page, false, {
    status: { link: "linked", signed_out: false, server: true, live: false, session: null },
    settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false },
    agent_accounts: { claude: null, codex_key: null, machines: [] },
    list_machines: [], list_credentials: [], list_github_accounts: [],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agent accounts", exact: true }).click();
  await settings.getByRole("button", { name: "Paste a token" }).click();
  const input = settings.getByRole("textbox", { name: "Claude Code token" });
  await input.fill("example-secret");
  await input.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(2, 5); Reflect.set(window, "agentInput", el); Reflect.set(window, "agentHost", el.closest("#set-agents")); } });
  await page.evaluate(() => window.__navigation.emit("link-state", "linked"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(1);
  await expect(input).toHaveValue("example-secret");
  expect(await input.evaluate(el => el === Reflect.get(window, "agentInput") && document.activeElement === el && el instanceof HTMLInputElement && el.selectionStart === 2 && el.selectionEnd === 5)).toBe(true);
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
    status: { link: "linked", signed_out: false, server: true, live: false, session: null },
    settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false },
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
