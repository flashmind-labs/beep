import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("bar Settings shares its page root and preserves controls on refresh", async ({ page }, info) => {
  await openDesktop(page, false, {
    status: { link: "unlinked", signed_out: false, server: true, live: false, session: null },
    settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false },
    list_devices: { devices: [], server: "answered" }, list_machines: [], list_apps: [], list_mcp_servers: [],
    agent_accounts: { claude: null, codex_key: null, machines: [] }, list_credentials: [], list_github_accounts: [],
  });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar > *").first()).toBeVisible();
  await page.keyboard.press(`${info.project.name === "Windows" ? "Control" : "Meta"}+,`);
  const general = page.locator("#set-general");
  await expect(general).toBeVisible();
  const hint = page.locator("#bar .dhints");
  await hint.evaluate(node => Reflect.set(window, "settingsHints", node));
  const control = general.getByRole("switch").first();
  await control.focus();
  await control.evaluate(node => Reflect.set(window, "settingsControl", node));
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.__navigation.emit("link-state", "unlinked"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(reads);
  expect(await control.evaluate(node => node === Reflect.get(window, "settingsControl") && node === document.activeElement)).toBe(true);
  expect(await hint.evaluate(node => node === Reflect.get(window, "settingsHints"))).toBe(true);
  await expect(page.locator("#bar > [data-bar-page]")).toHaveCount(1);
});

test("bar Apps and MCP reuse the Settings root", async ({ page }) => {
  await openDesktop(page, false, {
    status: { link: "linked", signed_out: false, server: true, live: false, session: null },
    settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false },
    list_devices: { devices: [], server: "answered" }, list_machines: [], list_apps: [],
    list_mcp_servers: [{ id: "notes", name: "Notes", url: "https://example.test/mcp", connected: true, sign_in: false, tools: ["search"] }],
    list_mcp_tools: { limit: 1, tools: [{ name: "search", description: "Search notes", enabled: true, usable: true }] },
    agent_accounts: { claude: null, codex_key: null, machines: [] }, list_credentials: [], list_github_accounts: [],
  });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar > *").first()).toBeVisible();
  await page.evaluate(() => window.__navigation.emit("bar-shown", "settings"));
  await expect(page.locator("#set-apps")).toBeVisible();
  await page.locator("#bar > [data-bar-page]").evaluate(node => Reflect.set(window, "settingsAppsRoot", node));
  await page.locator("#set-apps").getByRole("button", { name: "Manage…" }).click();
  await expect(page.getByText("Your MCP servers")).toBeVisible();
  await page.getByRole("button", { name: /Notes/ }).click();
  await expect(page.getByRole("switch", { name: "Use Search" })).toBeVisible();
  expect(await page.locator("#bar > [data-bar-page]").evaluate(node => node === Reflect.get(window, "settingsAppsRoot"))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.getByText("Your MCP servers")).toBeVisible();
});
