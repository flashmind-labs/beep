import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const machine = (id: string, online: boolean, can_host = true) => ({
  id, name: id, online, can_host, seen_at: 1, version: "0.9.0", env: null,
});
const buildBox = machine("build-box", true);
const codexRemote = { transport: "http", url: "https://tools.example.test/mcp", headers: {},
  env_http_headers: {}, bearer_token_env_var: "DOCS_TOKEN" };
const options = { required: true, startup_timeout_sec: 5.25, tool_timeout_sec: null, enabled_tools: null,
  disabled_tools: null, default_tools_approval_mode: "prompt" };
const codex = { name: "docs", settings: { connection: { remote: codexRemote }, enabled: true, codex: options, claude_oauth: null } };
const claude = { name: "example", settings: { connection: { stdio: {
  command: "example-mcp", args: ["two words", "--local"], env: { EXAMPLE: "value" }, cwd: null,
} }, enabled: true, codex: null, claude_oauth: null } };
const calls = (page: Page, command: string) => page.evaluate(
  name => window.__navigation.calls().filter(call => call.command === name).map(call => call.args), command,
);

async function openMachines(page: Page, responses: Record<string, unknown>) {
  await openDesktop(page, false, {
    settings: { name: buildBox.name, version: "0.9.0", autostart: false, agent_notices: false },
    ...responses,
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("dialog", { name: "Settings" }).getByRole("navigation", { name: "Settings pages" })
    .getByRole("button", { name: /^Machines/ }).click();
}

test("machine MCP servers are loaded, edited and applied for the selected harness", async ({ page }) => {
  await openMachines(page, {
    list_machines: [buildBox, machine("test-box", false)],
    machine_mcp_harnesses: ["codex", "claude"],
    machine_mcp_servers: [codex],
    remove_machine_mcp_server: [],
    apply_machine_mcp_server: [
      { name: "second-box", error: null }, { name: "test-box", error: "Machine is offline." },
    ],
  });
  await page.locator("[data-machine='build-box']").click();
  const section = page.getByRole("region", { name: "MCP servers" });
  const selector = section.getByRole("button", { name: "Harness Codex" });
  await expect(selector).toBeVisible();
  await selector.click();
  await expect(page.getByRole("menu", { name: "Harness" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(selector).toBeFocused();
  await expect(section.getByRole("button", { name: /^docs/ })).toBeEnabled();
  expect(await calls(page, "machine_mcp_servers")).toEqual([{ machineId: buildBox.id, harness: "codex" }]);

  const changed = { ...codex, settings: { ...codex.settings, connection: { remote: { ...codexRemote, headers: { "X-Example": "value" } } },
    enabled: false, codex: { ...options, enabled_tools: ["search"] } } };
  await page.evaluate(server => window.__navigation.respond("save_machine_mcp_server", { status: "saved", servers: [server] }), changed);
  await section.getByRole("button", { name: /^docs/ }).click();
  const editSheet = page.getByRole("dialog", { name: "Edit docs" });
  await expect(editSheet.getByRole("button", { name: "Transport Streamable HTTP" })).toBeVisible();
  await expect(editSheet.getByLabel("Server URL")).toHaveValue(codexRemote.url);
  await expect(editSheet.getByLabel("Bearer token variable")).toHaveValue("DOCS_TOKEN");
  await expect(editSheet.locator("textarea")).toHaveCount(0);
  const headers = editSheet.getByRole("group", { name: "Headers", exact: true });
  await headers.getByRole("button", { name: "+ Add header" }).click();
  await headers.getByLabel("Header name 1").fill("X-Example");
  await headers.getByLabel("Value 1").fill("value");
  await editSheet.getByRole("checkbox", { name: "Enabled" }).uncheck();
  await editSheet.getByText("Startup and tool settings", { exact: true }).click();
  await expect(editSheet.getByLabel("Startup timeout (seconds)")).toHaveValue("5.25");
  await expect(editSheet.getByRole("button", { name: "Default tool approval Prompt" })).toBeVisible();
  await editSheet.getByRole("checkbox", { name: "Only allow listed tools" }).check();
  await editSheet.getByRole("button", { name: "+ Add allowed tool", exact: true }).click();
  await editSheet.getByLabel("Tool name 1", { exact: true }).fill("search");
  await editSheet.getByRole("button", { name: "Save server" }).click();
  await expect(editSheet).toHaveCount(0);
  expect(await calls(page, "save_machine_mcp_server")).toEqual([{ machineId: buildBox.id, harness: "codex", previous: codex, server: changed }]);

  const more = section.getByRole("button", { name: "More for docs" });
  await more.click();
  await expect(page.getByRole("menu", { name: "docs" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(more).toBeFocused();
  await more.click();
  await page.getByRole("menuitem", { name: "Apply to all" }).click();
  const applySheet = page.getByRole("dialog", { name: "Apply docs to all machines?" });
  await expect(applySheet.getByText(/to Codex on other linked machines/)).toBeVisible();
  await applySheet.getByRole("button", { name: "Apply to all machines" }).click();
  const result = section.getByRole("status");
  await expect(result).toContainText("Applied to 1 of 2 other machines.");
  await expect(result).toContainText("test-box · Machine is offline.");
  expect(await calls(page, "apply_machine_mcp_server")).toEqual([{ machineId: buildBox.id, harness: "codex", name: "docs" }]);

  await page.evaluate(() => window.__navigation.respond("machine_mcp_servers", []));
  await selector.click();
  await page.getByRole("menuitemradio", { name: "Claude Code" }).click();
  await expect(section.getByRole("button", { name: "Harness Claude Code" })).toBeFocused();
  await expect(section.getByText("No user servers configured")).toBeVisible();
  await expect(section.getByRole("button", { name: /^docs/ })).toHaveCount(0);
  expect((await calls(page, "machine_mcp_servers")).at(-1)).toEqual({ machineId: buildBox.id, harness: "claude" });

  await page.evaluate(server => window.__navigation.respond("save_machine_mcp_server", { status: "saved", servers: [server] }), claude);
  await section.getByRole("button", { name: "Add server" }).click();
  const addSheet = page.getByRole("dialog", { name: "Add MCP server" });
  await addSheet.getByLabel("Name", { exact: true }).fill(claude.name);
  await addSheet.getByLabel("Command", { exact: true }).fill("example-mcp");
  const args = addSheet.getByRole("group", { name: "Arguments" });
  await args.getByRole("button", { name: "+ Add argument" }).click();
  await args.getByLabel("Argument 1", { exact: true }).fill("two words");
  await args.getByRole("button", { name: "+ Add argument" }).click();
  await args.getByLabel("Argument 2", { exact: true }).fill("--local");
  const env = addSheet.getByRole("group", { name: "Environment variables" });
  await env.getByRole("button", { name: "+ Add variable" }).click();
  await env.getByLabel("Variable name 1").fill("EXAMPLE");
  await env.getByLabel("Value 1").fill("value");
  await addSheet.getByRole("button", { name: "Save server" }).click();
  await expect(section.getByRole("button", { name: /^example/ })).toBeEnabled();
  expect((await calls(page, "save_machine_mcp_server")).at(-1)).toEqual({ machineId: buildBox.id, harness: "claude", previous: null, server: claude });

  await section.getByRole("button", { name: "More for example" }).click();
  await page.getByRole("menuitem", { name: "Remove" }).click();
  await page.getByRole("dialog", { name: "Remove example from Claude Code?" }).getByRole("button", { name: "Remove server" }).click();
  await expect(section.getByRole("button", { name: /^example/ })).toHaveCount(0);
  expect(await calls(page, "remove_machine_mcp_server")).toEqual([{ machineId: buildBox.id, harness: "claude", name: "example" }]);
});

test("a stale edit keeps the draft and refreshes the machine's MCP list", async ({ page }) => {
  const latest = { ...codex, settings: { ...codex.settings, connection: { remote: { ...codexRemote, url: "https://tools.example.test/new" } } } };
  await openMachines(page, {
    list_machines: [buildBox],
    machine_mcp_harnesses: ["codex"],
    machine_mcp_servers: [codex],
    save_machine_mcp_server: { status: "conflict", servers: [latest] },
  });
  await page.locator("[data-machine='build-box']").click();
  const row = page.getByRole("region", { name: "MCP servers" }).getByRole("button", { name: /^docs/ });
  await row.click();
  const editor = page.getByRole("dialog", { name: "Edit docs" });
  const url = editor.getByLabel("Server URL");
  await url.fill("https://tools.example.test/draft");
  await editor.getByRole("button", { name: "Save server" }).click();

  await expect(editor.getByRole("alert")).toContainText("This server changed");
  await expect(url).toHaveValue("https://tools.example.test/draft");
  await expect(editor.getByRole("button", { name: "Save server" })).toBeDisabled();
  expect((await calls(page, "save_machine_mcp_server"))[0]?.previous).toEqual(codex);

  await editor.getByRole("button", { name: "Cancel" }).click();
  await row.click();
  await expect(url).toHaveValue(latest.settings.connection.remote.url);
});

test("settings identify offline hosts and hide MCP management on controller-only devices", async ({ page }) => {
  await openMachines(page, { list_machines: [machine("test-box", false), machine("phone", true, false)] });
  await page.locator("[data-machine='test-box']").click();
  const section = page.getByRole("region", { name: "MCP servers" });
  await expect(section).toContainText("Connect this machine to manage its MCP servers.");
  expect(await calls(page, "machine_mcp_harnesses")).toEqual([]);
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: /^Machines/ }).click();
  await page.locator("[data-machine='phone']").click();
  await expect(settings.getByText("Controller", { exact: true })).toBeVisible();
  await expect(section).toHaveCount(0);
});

test("Antigravity offers its two transports and can disable a server", async ({ page }) => {
  const remote = { transport: "http", url: "https://tools.example.test/mcp", headers: {}, env_http_headers: {}, bearer_token_env_var: null };
  const saved = { name: "api", settings: { connection: { remote }, enabled: false, codex: null, claude_oauth: null } };
  await openMachines(page, {
    list_machines: [buildBox],
    machine_mcp_harnesses: ["agy"],
    machine_mcp_servers: [],
    save_machine_mcp_server: { status: "saved", servers: [saved] },
  });
  await page.locator("[data-machine='build-box']").click();
  const section = page.getByRole("region", { name: "MCP servers" });
  await expect(section.getByRole("button", { name: "Harness Antigravity" })).toBeVisible();
  await section.getByRole("button", { name: "Add server" }).click();
  const sheet = page.getByRole("dialog", { name: "Add MCP server" });
  await sheet.getByRole("button", { name: "Transport Stdio" }).click();
  await expect(page.getByRole("menuitemradio")).toHaveText(["Stdio", "Streamable HTTP"]);
  await page.keyboard.press("Escape");
  await expect(sheet).toBeVisible();
  await expect(sheet.getByLabel("Working directory")).toHaveCount(0);
  await sheet.getByLabel("Name", { exact: true }).fill("api");
  await sheet.getByRole("button", { name: "Transport Stdio" }).click();
  await page.getByRole("menuitemradio", { name: "Streamable HTTP" }).click();
  await sheet.getByLabel("Server URL").fill(remote.url);
  await expect(sheet.getByLabel("Bearer token variable")).toHaveCount(0);
  await expect(sheet.getByText("OAuth settings")).toHaveCount(0);
  await sheet.getByLabel("Enabled").uncheck();
  await sheet.getByRole("button", { name: "Save server" }).click();
  await expect(section.getByRole("button", { name: /^api/ })).toContainText("Disabled");
  expect(await calls(page, "save_machine_mcp_server")).toEqual([{ machineId: buildBox.id, harness: "agy", previous: null, server: saved }]);
});
