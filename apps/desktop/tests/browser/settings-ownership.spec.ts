import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const base = {
  settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
  list_machines: [], list_credentials: [], list_github_accounts: [],
  cli_logins: { github: { state: "missing" }, forgejo: [] },
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
  const reads = await count(page, "list_machines");
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => count(page, "list_machines")).toBeGreaterThan(reads);
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
