import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const one = { choices: [{ id: "one", name: "One", input_price: null, output_price: null, sees_images: false, default: true }, { id: "two", name: "Two", input_price: 1, output_price: 2, sees_images: true, default: false }], selected: "one", thinking: "low", auto: false, auto_available: true };
const app = { slug: "mail", name: "Mail", description: "Messages", logo_url: null, connected: true, configured: true, kind: "composio" };
const base = {
  status: { link: "linked", signed_out: false, server: true, live: false, session: null },
  settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false },
  list_machines: [], list_devices: { devices: [], server: "answered" }, list_credentials: [], list_github_accounts: [],
  cli_logins: { github: { state: "missing" }, forgejo: [] }, get_models: one, list_apps: [app], list_mcp_servers: [],
  agent_accounts: { claude: null, codex_key: null, machines: [] },
};

async function openSettings(page: Page, responses: Record<string, unknown> = base) {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  return page.getByRole("dialog", { name: "Settings" });
}
const count = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).length, command);

test("section hosts and focused controls survive answers that redraw the sheet", async ({ page }) => {
  const settings = await openSettings(page);
  const general = settings.locator("#set-general");
  const control = general.getByRole("switch").first();
  await control.focus();
  await general.evaluate(node => Reflect.set(window, "generalHost", node));
  await control.evaluate(node => Reflect.set(window, "generalControl", node));
  const reads = await count(page, "list_apps");
  await page.evaluate(() => window.__navigation.emit("link-state", "linked"));
  await expect.poll(() => count(page, "list_apps")).toBeGreaterThan(reads);
  expect(await general.evaluate(node => node === Reflect.get(window, "generalHost"))).toBe(true);
  expect(await control.evaluate(node => node === Reflect.get(window, "generalControl") && node === document.activeElement)).toBe(true);
});

test("a failed token save clears only the token and keeps the form focused", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("button", { name: "Credentials", exact: true }).click();
  await settings.getByRole("button", { name: "+ Add a Forgejo/Gitea server" }).click();
  await settings.getByLabel("Server URL").fill("https://git.example.test");
  const secret = settings.getByLabel("Token");
  await secret.fill("example-secret");
  await page.evaluate(() => window.__navigation.rejectOnce("save_credential"));
  await secret.press("Enter");
  await expect(settings.locator("#credErr")).toContainText("Requested test failure");
  await expect(secret).toHaveValue("");
  await expect(settings.getByLabel("Server URL")).toHaveValue("https://git.example.test");
});

test("a save answered after leaving Credentials does not show its error", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("button", { name: "Credentials", exact: true }).click();
  await settings.getByRole("button", { name: "+ Add a Forgejo/Gitea server" }).click();
  await settings.getByLabel("Server URL").fill("https://git.example.test");
  await settings.getByLabel("Token").fill("example-secret");
  await page.evaluate(() => { window.__navigation.holdNext("save_credential"); window.__navigation.rejectOnce("save_credential"); });
  await settings.getByLabel("Token").press("Enter");
  await expect.poll(() => count(page, "save_credential")).toBe(1);
  await settings.getByRole("button", { name: "General", exact: true }).click();
  await expect(settings.locator("#set-creds")).toHaveCount(0);
  const reads = await count(page, "list_credentials");
  await page.evaluate(() => window.__navigation.release("save_credential"));
  await expect.poll(() => count(page, "list_credentials")).toBeGreaterThan(reads);
  await settings.getByRole("button", { name: "Credentials", exact: true }).click();
  await expect(settings.locator("#credErr")).toHaveText("");
});

test("a model error answered after leaving Models is dropped", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("button", { name: "Models", exact: true }).click();
  await page.evaluate(() => { window.__navigation.holdNext("set_model"); window.__navigation.rejectOnce("set_model"); });
  await settings.getByLabel("Model", { exact: true }).selectOption("two");
  await expect.poll(() => count(page, "set_model")).toBe(1);
  await settings.getByRole("button", { name: "General", exact: true }).click();
  await page.evaluate(() => window.__navigation.release("set_model"));
  await settings.getByRole("button", { name: "Models", exact: true }).click();
  await expect(settings.getByLabel("Model", { exact: true })).toBeEnabled();
  await expect(settings.locator("#setModelErr")).toHaveText("");
});

test("Apps renders inside its page host and leaves it with the sheet", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("button", { name: /^Apps/ }).click();
  const host = settings.locator("#set-apps-host");
  await expect(host.locator("#apps-react #alist")).toBeVisible();
  await host.evaluate(node => Reflect.set(window, "appsHost", node));
  await settings.locator("#apps-react").evaluate(node => Reflect.set(window, "appsView", node));
  const reads = await count(page, "list_apps");
  await page.evaluate(() => window.__navigation.emit("mcp-signed-in", null));
  await expect.poll(() => count(page, "list_apps")).toBeGreaterThan(reads);
  expect(await settings.locator("#apps-react").evaluate(node => node === Reflect.get(window, "appsView"))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  expect(await page.evaluate(() => { const node: unknown = Reflect.get(window, "appsHost"); return node instanceof HTMLElement ? node.childElementCount : -1; })).toBe(0);
});

test("the bar leaves Models out when the server offers no choice", async ({ page }) => {
  await openDesktop(page, false, { ...base, get_models: { ...one, choices: [] } });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar > *").first()).toBeVisible();
  await page.evaluate(() => window.__navigation.emit("bar-shown", "settings"));
  await expect(page.locator("#set-general")).toBeVisible();
  await expect.poll(() => count(page, "get_models")).toBeGreaterThan(0);
  await expect(page.locator("#set-models-sec")).toHaveCount(0);
  await expect(page.locator("#set-apps")).toBeVisible();
});
