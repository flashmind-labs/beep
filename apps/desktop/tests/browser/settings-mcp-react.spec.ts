import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const server = { id: "notes", name: "Notes", url: "https://example.test/mcp", connected: true, sign_in: false, tools: ["search", "write"] };
const tools = { limit: 1, tools: [{ name: "search", description: "Search notes", enabled: true, usable: true }, { name: "write", description: "Write notes", enabled: false, usable: true }] };
async function openServer(page: import("@playwright/test").Page) {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_apps: [], list_mcp_servers: [server], list_mcp_tools: tools });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Apps", exact: true }).click();
  await settings.getByRole("button", { name: /Notes/ }).click();
  await expect(settings.getByRole("switch", { name: "Use Search" })).toBeVisible();
  return settings;
}
test("MCP tools update in place, confirm limit and remove", async ({ page }) => {
  const settings = await openServer(page);
  const search = settings.getByRole("switch", { name: "Use Search" });
  await search.focus();
  await search.evaluate(el => Reflect.set(window, "searchSwitch", el));
  await page.evaluate(value => window.__navigation.respond("set_mcp_tool", value), { ...tools, tools: tools.tools.map(tool => tool.name === "search" ? { ...tool, enabled: false } : tool) });
  await search.click();
  await expect(search).toHaveAttribute("aria-checked", "false");
  expect(await search.evaluate(el => el === Reflect.get(window, "searchSwitch") && document.activeElement === el)).toBe(true);
  await page.evaluate(value => window.__navigation.respond("set_mcp_tool", value), tools);
  await search.click();
  await expect(search).toHaveAttribute("aria-checked", "true");
  await settings.getByRole("switch", { name: "Use Write" }).click();
  await expect(settings.getByText("More than 1 tools makes replies slower and tool picks worse")).toBeVisible();
  await settings.getByRole("button", { name: "Turn on" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_mcp_tool" && call.args.tool === "write" && call.args.enabled === true))).toBe(true);
  await settings.getByRole("button", { name: "Remove server" }).click();
  await expect(settings.getByText("Beep stops using this server")).toBeVisible();
  await settings.getByRole("button", { name: "Remove", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "remove_mcp_server" && call.args.id === "notes"))).toBe(true);
  await expect(settings.getByText("Your MCP servers")).toBeVisible();
});

test("late tool results do not replace a reopened server", async ({ page }) => {
  const settings = await openServer(page);
  await page.evaluate(() => window.__navigation.hold("set_mcp_tool"));
  await page.evaluate(value => window.__navigation.respond("set_mcp_tool", value), { ...tools, tools: tools.tools.map(tool => ({ ...tool, enabled: false })) });
  await settings.getByRole("switch", { name: "Use Search" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_mcp_tool"))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(settings.getByText("Your MCP servers")).toBeVisible();
  await settings.getByRole("button", { name: /Notes/ }).click();
  const search = settings.getByRole("switch", { name: "Use Search" });
  await expect(search).toHaveAttribute("aria-checked", "true");
  await page.evaluate(() => window.__navigation.release("set_mcp_tool"));
  await expect(search).toHaveAttribute("aria-checked", "true");
});
