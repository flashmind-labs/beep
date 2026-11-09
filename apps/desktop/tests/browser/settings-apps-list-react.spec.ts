import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const app = { slug: "mail", name: "Mail", description: "Messages", logo_url: null, connected: true, configured: true, kind: "composio" };
const server = { id: "notes", name: "Notes", url: "https://example.test/mcp", connected: true, sign_in: false, tools: ["search"] };
test("live list refresh keeps MCP draft, focus and selection", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_apps: [app], list_mcp_servers: [server], add_mcp_server: server });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: /^Apps/ }).click();
  await settings.getByRole("button", { name: "Add MCP server" }).click();
  const name = settings.getByRole("textbox", { name: "Name" }), url = settings.getByRole("textbox", { name: "Server URL" });
  await name.fill("My notes");
  await url.fill("https://example.test/mcp");
  await url.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(3, 7); Reflect.set(window, "mcpDraft", el); } });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "list_mcp_servers").length);
  await page.evaluate(() => window.__navigation.emit("mcp-signed-in", null));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "list_mcp_servers").length)).toBeGreaterThan(reads);
  await expect(name).toHaveValue("My notes");
  await expect(url).toHaveValue("https://example.test/mcp");
  expect(await url.evaluate(el => el === Reflect.get(window, "mcpDraft") && document.activeElement === el && el instanceof HTMLInputElement && el.selectionStart === 3 && el.selectionEnd === 7)).toBe(true);
  await url.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "add_mcp_server").map(call => call.args))).toEqual([{ name: "My notes", url: "https://example.test/mcp" }]);
});

test("connected list rows open with the keyboard", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_apps: [app], list_mcp_servers: [server], list_mcp_tools: { limit: 5, tools: [] } });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: /^Apps/ }).click();
  await settings.locator("[data-mopen=notes]").focus();
  await page.keyboard.press("Enter");
  await expect(settings.getByText("0 of 5 on")).toBeVisible();
});

test("pending MCP add completes after Settings is reopened", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_apps: [app], list_mcp_servers: [server], add_mcp_server: server });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  let settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: /^Apps/ }).click();
  await settings.getByRole("button", { name: "Add MCP server" }).click();
  await settings.getByRole("textbox", { name: "Name" }).fill("My notes");
  await settings.getByRole("textbox", { name: "Server URL" }).fill("https://example.test/mcp");
  await page.evaluate(() => window.__navigation.hold("add_mcp_server"));
  await settings.getByRole("button", { name: /Add server/ }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "add_mcp_server").length)).toBe(1);
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("textbox", { name: "Name" })).toHaveValue("My notes");
  await expect(settings.getByRole("textbox", { name: "Name" })).toBeDisabled();
  await page.evaluate(() => window.__navigation.release("add_mcp_server"));
  await expect(settings.getByRole("button", { name: "Add MCP server" })).toBeVisible();
  await expect(settings.getByRole("textbox", { name: "Name" })).toHaveCount(0);
});

test("an updated app logo retries after a previous image failed", async ({ page }) => {
  const broken = { ...app, logo_url: "https://app.test/missing.svg" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_apps: [broken], list_mcp_servers: [server] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: /^Apps/ }).click();
  const row = settings.locator("[data-open=mail]");
  await expect(row.locator("span.dalogo")).toHaveText("M");
  const logo = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10'/%3E";
  await page.evaluate(value => { window.__navigation.respond("list_apps", [value]); window.__navigation.emit("mcp-signed-in", null); }, { ...app, logo_url: logo });
  await expect(row.locator("img.dalogo")).toHaveAttribute("src", logo);
  await expect.poll(() => row.locator("img.dalogo").evaluate(el => el instanceof HTMLImageElement && el.complete && el.naturalWidth > 0)).toBe(true);
});
